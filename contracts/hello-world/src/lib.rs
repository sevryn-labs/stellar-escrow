// Soroban Escrow Contract
// This contract implements a secure escrow system where:
// - A buyer locks tokens with a seller and arbiter
// - Payment can be released, refunded, or disputed
// - An arbiter can resolve disputes

#![no_std]  // Don't use standard library (required for Soroban)
use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, token, Address, Env, Vec,
};

// ============================================================================
// DATA STRUCTURES
// ============================================================================

/// Keys used to store data in the contract's persistent storage
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Escrow(u64),      // Stores individual escrow by ID
    Counter,          // Tracks the total number of escrows created
}

/// Main escrow data structure
/// Contains all information about a single escrow transaction
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Escrow {
    pub id: u64,              // Unique escrow identifier
    pub buyer: Address,       // Person who locks the funds
    pub seller: Address,      // Person who will receive the funds
    pub arbiter: Address,     // Third party who can resolve disputes
    pub token: Address,       // Token contract address (e.g., USDC, XLM)
    pub amount: i128,         // Amount of tokens locked in escrow
    pub deadline: u64,        // Unix timestamp when auto-refund is possible
    pub status: EscrowStatus, // Current state of the escrow
}

/// Possible states an escrow can be in
/// This prevents invalid state transitions (e.g., releasing already refunded escrow)
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub enum EscrowStatus {
    Active,    // Escrow is active, waiting for action
    Released,  // Funds have been released to seller
    Refunded,  // Funds have been returned to buyer
    Disputed,  // Escrow is disputed, awaiting arbiter decision
}

/// Custom error codes for the contract
/// Makes debugging easier and provides clear error messages
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    EscrowNotFound = 1,  // Tried to access non-existent escrow
    NotActive = 2,       // Escrow is not in Active state
    NotAuthorized = 3,   // Caller doesn't have permission
    NotDisputed = 4,     // Tried to resolve non-disputed escrow
}

// ============================================================================
// CONTRACT IMPLEMENTATION
// ============================================================================

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    
    // ========================================================================
    // CREATE ESCROW
    // ========================================================================
    /// Creates a new escrow and locks tokens from the buyer
    /// 
    /// Flow:
    /// 1. Buyer authorizes the transaction
    /// 2. Generate unique escrow ID
    /// 3. Store escrow details
    /// 4. Transfer tokens from buyer to contract
    /// 
    /// Returns: The new escrow ID
    pub fn create_escrow(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        token: Address,
        amount: i128,
        deadline: u64,  // Unix timestamp
    ) -> u64 {
        // AUTHORIZATION: Verify the buyer is signing this transaction
        // This ensures only the buyer can create an escrow with their funds
        buyer.require_auth();

        // COUNTER MANAGEMENT: Get the current escrow count and increment it
        let counter_key = DataKey::Counter;
        let escrow_id: u64 = env
            .storage()
            .instance()  // Instance storage persists between contract calls
            .get(&counter_key)
            .unwrap_or(0);  // Start from 0 if no escrows exist yet
        
        let new_id = escrow_id + 1;
        env.storage().instance().set(&counter_key, &new_id);

        // CREATE ESCROW: Build the escrow data structure
        let escrow = Escrow {
            id: new_id,
            buyer: buyer.clone(),
            seller: seller.clone(),
            arbiter,
            token: token.clone(),
            amount,
            deadline,
            status: EscrowStatus::Active,  // New escrows always start as Active
        };

        // STORE: Save the escrow to persistent storage
        env.storage()
            .instance()
            .set(&DataKey::Escrow(new_id), &escrow);

        // TOKEN TRANSFER: Move tokens from buyer to this contract
        // The contract now holds the tokens in escrow
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&buyer, &env.current_contract_address(), &amount);

        // Return the new escrow ID so the buyer can track it
        new_id
    }

    // ========================================================================
    // RELEASE PAYMENT
    // ========================================================================
    /// Buyer releases the locked funds to the seller
    /// This is the "happy path" - buyer is satisfied with the service/product
    /// 
    /// Requirements:
    /// - Only the buyer can release
    /// - Escrow must be in Active state
    pub fn release_payment(env: Env, escrow_id: u64) -> Result<(), Error> {
        // LOAD ESCROW: Retrieve escrow from storage
        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;  // Return error if escrow doesn't exist

        // AUTHORIZATION: Only the buyer can release payment
        escrow.buyer.require_auth();

        // STATE CHECK: Ensure escrow is still active
        // Can't release if already released, refunded, or disputed
        if escrow.status != EscrowStatus::Active {
            return Err(Error::NotActive);
        }

        // TRANSFER: Send tokens from contract to seller
        let token_client = token::Client::new(&env, &escrow.token);
        token_client.transfer(
            &env.current_contract_address(),  // From: this contract
            &escrow.seller,                   // To: seller
            &escrow.amount,
        );

        // UPDATE STATE: Mark escrow as released
        escrow.status = EscrowStatus::Released;
        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    // ========================================================================
    // REFUND PAYMENT
    // ========================================================================
    /// Returns locked funds back to the buyer
    /// Can be triggered by:
    /// 1. Anyone after the deadline has passed (automatic refund)
    /// 2. Arbiter at any time (manual intervention)
    /// 
    /// Requirements:
    /// - Escrow must be Active
    /// - Either past deadline OR caller is arbiter
    pub fn refund_payment(env: Env, escrow_id: u64, caller: Address) -> Result<(), Error> {
        // AUTHORIZATION: Verify the caller is who they claim to be
        caller.require_auth();

        // LOAD ESCROW
        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        // STATE CHECK: Can only refund active escrows
        if escrow.status != EscrowStatus::Active {
            return Err(Error::NotActive);
        }

        // AUTHORIZATION CHECK: Determine if refund is allowed
        let current_time = env.ledger().timestamp();  // Get blockchain time
        let is_past_deadline = current_time > escrow.deadline;
        let is_arbiter = caller == escrow.arbiter;

        // Refund is allowed if EITHER condition is true:
        // - Deadline has passed (automatic refund protection)
        // - Caller is the arbiter (manual intervention)
        if !is_past_deadline && !is_arbiter {
            return Err(Error::NotAuthorized);
        }

        // TRANSFER: Return tokens to buyer
        let token_client = token::Client::new(&env, &escrow.token);
        token_client.transfer(
            &env.current_contract_address(),
            &escrow.buyer,
            &escrow.amount,
        );

        // UPDATE STATE: Mark as refunded
        escrow.status = EscrowStatus::Refunded;
        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    // ========================================================================
    // DISPUTE ESCROW
    // ========================================================================
    /// Either buyer or seller can dispute an escrow
    /// This locks the escrow and requires arbiter intervention
    /// 
    /// Use cases:
    /// - Seller didn't deliver
    /// - Buyer won't release payment unfairly
    /// - Quality issues
    pub fn dispute_escrow(env: Env, escrow_id: u64, caller: Address) -> Result<(), Error> {
        caller.require_auth();

        // LOAD ESCROW
        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        // AUTHORIZATION: Only buyer or seller can dispute
        // Arbiter cannot initiate disputes, only resolve them
        if caller != escrow.buyer && caller != escrow.seller {
            return Err(Error::NotAuthorized);
        }

        // STATE CHECK: Can only dispute active escrows
        if escrow.status != EscrowStatus::Active {
            return Err(Error::NotActive);
        }

        // UPDATE STATE: Mark as disputed
        // This prevents release/refund until arbiter resolves
        escrow.status = EscrowStatus::Disputed;
        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    // ========================================================================
    // RESOLVE DISPUTE
    // ========================================================================
    /// Arbiter decides where the funds should go
    /// This is the final decision mechanism
    /// 
    /// Parameters:
    /// - release_to_seller: true = seller wins, false = buyer wins
    pub fn resolve_dispute(
        env: Env,
        escrow_id: u64,
        release_to_seller: bool,
    ) -> Result<(), Error> {
        // LOAD ESCROW
        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        // AUTHORIZATION: Only arbiter can resolve disputes
        escrow.arbiter.require_auth();

        // STATE CHECK: Can only resolve disputed escrows
        if escrow.status != EscrowStatus::Disputed {
            return Err(Error::NotDisputed);
        }

        // TRANSFER: Send funds based on arbiter's decision
        let token_client = token::Client::new(&env, &escrow.token);

        if release_to_seller {
            // Arbiter sides with seller - release payment
            token_client.transfer(
                &env.current_contract_address(),
                &escrow.seller,
                &escrow.amount,
            );
            escrow.status = EscrowStatus::Released;
        } else {
            // Arbiter sides with buyer - refund payment
            token_client.transfer(
                &env.current_contract_address(),
                &escrow.buyer,
                &escrow.amount,
            );
            escrow.status = EscrowStatus::Refunded;
        }

        // UPDATE STATE
        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    // ========================================================================
    // QUERY FUNCTIONS (READ-ONLY)
    // ========================================================================
    
    /// Get full details of a specific escrow
    /// This is a read-only function (no state changes)
    pub fn get_escrow(env: Env, escrow_id: u64) -> Result<Escrow, Error> {
        let key = DataKey::Escrow(escrow_id);
        env.storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)
    }

    /// Get all escrow IDs where the user is buyer or seller
    /// Useful for building user dashboards
    /// 
    /// Note: This iterates through all escrows - in production,
    /// you'd want to use indexed storage for better performance
    pub fn get_user_escrows(env: Env, user: Address) -> Vec<u64> {
        // Get total number of escrows
        let counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::Counter)
            .unwrap_or(0);

        let mut escrow_ids = Vec::new(&env);

        // Iterate through all escrows and find matches
        for i in 1..=counter {
            if let Some(escrow) = env
                .storage()
                .instance()
                .get::<DataKey, Escrow>(&DataKey::Escrow(i))
            {
                // Include escrow if user is buyer or seller
                if escrow.buyer == user || escrow.seller == user {
                    escrow_ids.push_back(i);
                }
            }
        }

        escrow_ids
    }
}

// ============================================================================
// TESTS
// ============================================================================
// Tests run off-chain to verify contract logic before deployment

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        token::{StellarAssetClient, TokenClient},
        Address, Env,
    };

    /// Helper function to create a test token
    fn create_token_contract<'a>(env: &Env, admin: &Address) -> (TokenClient<'a>, Address) {
        let contract = env.register_stellar_asset_contract_v2(admin.clone());
        (
            TokenClient::new(env, &contract.address()),
            contract.address(),
        )
    }

    #[test]
    fn test_create_and_release_escrow() {
        // Setup test environment
        let env = Env::default();
        env.mock_all_auths();  // Bypass authorization checks for testing

        // Deploy escrow contract
        let contract_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &contract_id);

        // Create test accounts
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let arbiter = Address::generate(&env);
        let token_admin = Address::generate(&env);

        // Create token and give buyer some tokens
        let (token_client, token_address) = create_token_contract(&env, &token_admin);
        let stellar_asset = StellarAssetClient::new(&env, &token_address);
        stellar_asset.mint(&buyer, &1000);

        // TEST: Create escrow
        let deadline = env.ledger().timestamp() + 86400; // 1 day from now
        let escrow_id = client.create_escrow(
            &buyer,
            &seller,
            &arbiter,
            &token_address,
            &500,
            &deadline,
        );

        // Verify escrow was created with ID 1
        assert_eq!(escrow_id, 1);

        // Verify escrow details
        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.amount, 500);
        assert_eq!(escrow.status, EscrowStatus::Active);

        // TEST: Release payment
        client.release_payment(&escrow_id);

        // Verify status changed and seller received tokens
        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Released);
        assert_eq!(token_client.balance(&seller), 500);
    }

    #[test]
    fn test_refund_after_deadline() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &contract_id);

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let arbiter = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let (token_client, token_address) = create_token_contract(&env, &token_admin);
        let stellar_asset = StellarAssetClient::new(&env, &token_address);
        stellar_asset.mint(&buyer, &1000);

        // Create escrow with short deadline
        let deadline = env.ledger().timestamp() + 100;
        let escrow_id = client.create_escrow(
            &buyer,
            &seller,
            &arbiter,
            &token_address,
            &500,
            &deadline,
        );

        // TEST: Time travel past deadline
        env.ledger().with_mut(|li| li.timestamp = deadline + 1);

        // TEST: Refund should now work
        client.refund_payment(&escrow_id, &buyer);

        // Verify refund
        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Refunded);
        assert_eq!(token_client.balance(&buyer), 1000);  // Got all tokens back
    }

    #[test]
    fn test_dispute_resolution() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &contract_id);

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let arbiter = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let (token_client, token_address) = create_token_contract(&env, &token_admin);
        let stellar_asset = StellarAssetClient::new(&env, &token_address);
        stellar_asset.mint(&buyer, &1000);

        let deadline = env.ledger().timestamp() + 86400;
        let escrow_id = client.create_escrow(
            &buyer,
            &seller,
            &arbiter,
            &token_address,
            &500,
            &deadline,
        );

        // TEST: Buyer disputes the escrow
        client.dispute_escrow(&escrow_id, &buyer);

        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Disputed);

        // TEST: Arbiter resolves in favor of seller
        client.resolve_dispute(&escrow_id, &true);

        // Verify resolution
        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Released);
        assert_eq!(token_client.balance(&seller), 500);
    }
}
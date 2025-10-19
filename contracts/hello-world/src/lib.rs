#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, token, Address, Env, Vec,
};

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Escrow(u64),
    Counter,
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Escrow {
    pub id: u64,
    pub buyer: Address,
    pub seller: Address,
    pub arbiter: Address,
    pub token: Address,
    pub amount: i128,
    pub deadline: u64,
    pub status: EscrowStatus,
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub enum EscrowStatus {
    Active,
    Released,
    Refunded,
    Disputed,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    EscrowNotFound = 1,
    NotActive = 2,
    NotAuthorized = 3,
    NotDisputed = 4,
}

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Create a new escrow
    pub fn create_escrow(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        token: Address,
        amount: i128,
        deadline: u64,
    ) -> u64 {
        // Verify buyer authorization
        buyer.require_auth();

        // Get and increment escrow counter
        let counter_key = DataKey::Counter;
        let escrow_id: u64 = env
            .storage()
            .instance()
            .get(&counter_key)
            .unwrap_or(0);
        
        let new_id = escrow_id + 1;
        env.storage().instance().set(&counter_key, &new_id);

        // Create escrow struct
        let escrow = Escrow {
            id: new_id,
            buyer: buyer.clone(),
            seller: seller.clone(),
            arbiter,
            token: token.clone(),
            amount,
            deadline,
            status: EscrowStatus::Active,
        };

        // Store escrow
        env.storage()
            .instance()
            .set(&DataKey::Escrow(new_id), &escrow);

        // Transfer tokens from buyer to contract
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&buyer, &env.current_contract_address(), &amount);

        new_id
    }

    /// Release payment to seller (buyer approves)
    pub fn release_payment(env: Env, escrow_id: u64) -> Result<(), Error> {
        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        // Verify buyer authorization
        escrow.buyer.require_auth();

        // Check escrow is active
        if escrow.status != EscrowStatus::Active {
            return Err(Error::NotActive);
        }

        // Transfer tokens to seller
        let token_client = token::Client::new(&env, &escrow.token);
        token_client.transfer(
            &env.current_contract_address(),
            &escrow.seller,
            &escrow.amount,
        );

        // Update status
        escrow.status = EscrowStatus::Released;
        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    /// Refund payment to buyer (after deadline or by arbiter)
    pub fn refund_payment(env: Env, escrow_id: u64, caller: Address) -> Result<(), Error> {
        caller.require_auth();

        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        // Check escrow is active
        if escrow.status != EscrowStatus::Active {
            return Err(Error::NotActive);
        }

        // Check authorization: either past deadline or arbiter
        let current_time = env.ledger().timestamp();
        let is_past_deadline = current_time > escrow.deadline;
        let is_arbiter = caller == escrow.arbiter;

        if !is_past_deadline && !is_arbiter {
            return Err(Error::NotAuthorized);
        }

        // Transfer tokens back to buyer
        let token_client = token::Client::new(&env, &escrow.token);
        token_client.transfer(
            &env.current_contract_address(),
            &escrow.buyer,
            &escrow.amount,
        );

        // Update status
        escrow.status = EscrowStatus::Refunded;
        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    /// Dispute escrow (marks for arbiter review)
    pub fn dispute_escrow(env: Env, escrow_id: u64, caller: Address) -> Result<(), Error> {
        caller.require_auth();

        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        // Only buyer or seller can dispute
        if caller != escrow.buyer && caller != escrow.seller {
            return Err(Error::NotAuthorized);
        }

        // Check escrow is active
        if escrow.status != EscrowStatus::Active {
            return Err(Error::NotActive);
        }

        // Update status
        escrow.status = EscrowStatus::Disputed;
        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    /// Arbiter resolves dispute
    pub fn resolve_dispute(
        env: Env,
        escrow_id: u64,
        release_to_seller: bool,
    ) -> Result<(), Error> {
        let key = DataKey::Escrow(escrow_id);
        let mut escrow: Escrow = env
            .storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        // Verify arbiter authorization
        escrow.arbiter.require_auth();

        // Check escrow is disputed
        if escrow.status != EscrowStatus::Disputed {
            return Err(Error::NotDisputed);
        }

        let token_client = token::Client::new(&env, &escrow.token);

        if release_to_seller {
            // Release to seller
            token_client.transfer(
                &env.current_contract_address(),
                &escrow.seller,
                &escrow.amount,
            );
            escrow.status = EscrowStatus::Released;
        } else {
            // Refund to buyer
            token_client.transfer(
                &env.current_contract_address(),
                &escrow.buyer,
                &escrow.amount,
            );
            escrow.status = EscrowStatus::Refunded;
        }

        env.storage().instance().set(&key, &escrow);

        Ok(())
    }

    /// Get escrow details
    pub fn get_escrow(env: Env, escrow_id: u64) -> Result<Escrow, Error> {
        let key = DataKey::Escrow(escrow_id);
        env.storage()
            .instance()
            .get(&key)
            .ok_or(Error::EscrowNotFound)
    }

    /// Get all escrows for a user (buyer or seller)
    pub fn get_user_escrows(env: Env, user: Address) -> Vec<u64> {
        let counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::Counter)
            .unwrap_or(0);

        let mut escrow_ids = Vec::new(&env);

        for i in 1..=counter {
            if let Some(escrow) = env
                .storage()
                .instance()
                .get::<DataKey, Escrow>(&DataKey::Escrow(i))
            {
                if escrow.buyer == user || escrow.seller == user {
                    escrow_ids.push_back(i);
                }
            }
        }

        escrow_ids
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        token::{StellarAssetClient, TokenClient},
        Address, Env,
    };

    fn create_token_contract<'a>(env: &Env, admin: &Address) -> (TokenClient<'a>, Address) {
        let contract = env.register_stellar_asset_contract_v2(admin.clone());
        (
            TokenClient::new(env, &contract.address()),
            contract.address(),
        )
    }

    #[test]
    fn test_create_and_release_escrow() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &contract_id);

        // Create accounts
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let arbiter = Address::generate(&env);
        let token_admin = Address::generate(&env);

        // Create and mint tokens
        let (token_client, token_address) = create_token_contract(&env, &token_admin);
        let stellar_asset = StellarAssetClient::new(&env, &token_address);
        stellar_asset.mint(&buyer, &1000);

        // Create escrow
        let deadline = env.ledger().timestamp() + 86400; // 1 day
        let escrow_id = client.create_escrow(
            &buyer,
            &seller,
            &arbiter,
            &token_address,
            &500,
            &deadline,
        );

        assert_eq!(escrow_id, 1);

        // Check escrow details
        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.amount, 500);
        assert_eq!(escrow.status, EscrowStatus::Active);

        // Release payment
        client.release_payment(&escrow_id);

        // Verify status and balances
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

        // Move time past deadline
        env.ledger().with_mut(|li| li.timestamp = deadline + 1);

        // Refund
        client.refund_payment(&escrow_id, &buyer);

        // Verify refund
        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Refunded);
        assert_eq!(token_client.balance(&buyer), 1000);
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

        // Dispute escrow
        client.dispute_escrow(&escrow_id, &buyer);

        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Disputed);

        // Arbiter resolves in favor of seller
        client.resolve_dispute(&escrow_id, &true);

        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Released);
        assert_eq!(token_client.balance(&seller), 500);
    }
}
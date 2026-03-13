# Soroban Escrow Contract

A **secure escrow smart contract for Soroban (Stellar smart contracts)** that enables trust-minimized payments between a buyer and seller with optional dispute resolution through an arbiter.

The contract locks tokens on-chain and ensures funds are released only when predefined conditions are met.

---

## Overview

Escrow is a fundamental financial primitive used when two parties transact without fully trusting each other.

This contract implements a **three-party escrow system**:

* **Buyer** — locks funds into the escrow
* **Seller** — receives funds when conditions are satisfied
* **Arbiter** — resolves disputes if conflicts arise

The contract supports:

* Conditional payment release
* Automatic refunds after deadlines
* Dispute escalation and arbitration
* Token-agnostic escrows (works with any Soroban token)

---

## Escrow Lifecycle

An escrow moves through a well-defined state machine.

```
Active
  │
  ├── Buyer releases payment ───────► Released
  │
  ├── Deadline passes ─────────────► Refunded
  │
  └── Buyer/Seller disputes ───────► Disputed
                                      │
                                      └── Arbiter resolves
                                              ├── Released
                                              └── Refunded
```

States are strictly enforced to prevent invalid transitions.

---

## Core Features

### Token Locking

Funds are transferred from the buyer into the contract when the escrow is created.

The contract holds custody of tokens until a resolution occurs.

---

### Conditional Payment Release

The buyer can release funds to the seller once the service or product has been delivered.

---

### Automatic Refund Protection

Each escrow includes a **deadline timestamp**.

If the deadline passes without payment release:

* Anyone can trigger a refund
* Funds return to the buyer

This prevents sellers from locking funds indefinitely.

---

### Dispute Resolution

Either the buyer or seller can escalate a transaction into dispute.

Once disputed:

* Escrow becomes locked
* Only the arbiter can resolve the outcome

The arbiter decides whether funds go to:

* Seller (payment release)
* Buyer (refund)

---

## Contract Structure

### Escrow Object

Each escrow stores the following data:

```
Escrow {
  id
  buyer
  seller
  arbiter
  token
  amount
  deadline
  status
}
```

### Escrow Status

Possible states:

```
Active
Released
Refunded
Disputed
```

These ensure the contract behaves as a **strict state machine**.

---

## Contract Functions

### Create Escrow

Locks funds from the buyer and initializes the escrow.

```
create_escrow(
  buyer,
  seller,
  arbiter,
  token,
  amount,
  deadline
)
```

Returns the escrow ID.

---

### Release Payment

Allows the buyer to release funds to the seller.

```
release_payment(escrow_id)
```

Requirements:

* Caller must be the buyer
* Escrow must be active

---

### Refund Payment

Returns funds to the buyer.

```
refund_payment(escrow_id, caller)
```

Allowed when:

* Deadline has passed
  or
* Caller is the arbiter

---

### Dispute Escrow

Buyer or seller escalates the escrow to arbitration.

```
dispute_escrow(escrow_id, caller)
```

---

### Resolve Dispute

Arbiter decides where funds go.

```
resolve_dispute(
  escrow_id,
  release_to_seller
)
```

---

### Query Functions

Retrieve escrow information.

```
get_escrow(escrow_id)
get_user_escrows(user)
```

---

## Example Flow

1. Buyer creates escrow and locks tokens.

```
create_escrow(...)
```

2. Seller delivers service.

3. Buyer releases payment.

```
release_payment(escrow_id)
```

Or if something goes wrong:

4. Either party disputes.

```
dispute_escrow(escrow_id)
```

5. Arbiter resolves.

```
resolve_dispute(...)
```

---

## Testing

The contract includes unit tests that verify:

* Escrow creation
* Payment release
* Deadline refunds
* Dispute resolution

Tests simulate token minting and contract interactions using the Soroban test environment.

---

## Security Considerations

* Authorization checks enforce role permissions.
* Escrow state transitions prevent invalid operations.
* Deadline-based refunds prevent locked funds.
* Arbiter resolution allows off-chain dispute handling.

---

## Potential Improvements

For production deployments, the following could be added:

* Indexed escrow storage for faster queries
* Event emissions for escrow lifecycle tracking
* Partial payouts or milestone escrows
* Multiple arbiters or DAO arbitration
* Fee mechanism for the escrow service

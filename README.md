# MedChain QC

> **Tamper-proof medical record access control on Stellar — built for patients in Quezon City and beyond.**

---

## Problem

A diabetic patient in Quezon City changes clinics frequently and repeatedly pays for duplicate laboratory tests because previous medical records are stored in incompatible hospital systems. There is no patient-controlled, portable, verifiable record layer — paper records get lost, digital records stay siloed, and patients pay the price (literally).

## Solution

Patients upload SHA-256 hashes of encrypted medical records to Stellar via Soroban smart contracts. The actual files remain encrypted off-chain (IPFS or similar). The on-chain contract controls doctor access permissions and maintains a tamper-proof audit log of every upload, grant, revoke, and read — all tied to Stellar wallet addresses.

**Why Stellar specifically:**
- ~5 second finality means access revocation is near-instant (vs. Ethereum's minutes)
- Sub-cent transaction fees make per-record uploads viable for low-income patients
- Soroban's native auth model ties every operation to wallet signatures — no separate auth layer needed
- USDC on Stellar enables future premium features (paying for record verification, clinic subscriptions)

---

## Stellar Features Used

| Feature | Why |
|---|---|
| **Soroban smart contracts** | Record storage, access control, audit log — all on-chain logic |
| **Stellar wallet auth** | `require_auth()` enforces that only the patient can upload/grant/revoke |
| **XLM** | Transaction fees (sub-cent per operation) |
| **USDC (optional)** | Future: pay clinics a micro-fee for verified record uploads |

---

## Target Users

| Attribute | Detail |
|---|---|
| **Who** | Diabetic / chronic illness patients, 25–60 years old, visiting multiple clinics |
| **Where** | Quezon City, NCR Philippines (and any multi-clinic SEA urban patient) |
| **Pain** | Spending ₱500–₱2,000 per duplicate lab test every time they switch doctors |
| **Incentive** | Own and control their records; show any doctor instantly without repeat tests |

---

## Core Feature — MVP Transaction Flow

```
Patient uploads CBC hash
  → upload_record(patient, hash, "CBC 2025-03-15", clinic_address)
  → RecordEntry stored persistently on-chain
  → AuditLog: [UPLOAD] appended

Patient grants Dr. Santos access
  → grant_access(patient, dr_santos_address)
  → Access(patient, dr_santos) = true
  → AuditLog: [GRANT] appended

Dr. Santos reads records at new clinic
  → read_records(dr_santos, patient)
  → Contract checks Access(patient, dr_santos) == true
  → Returns Vec<RecordEntry> with all hashes + labels
  → AuditLog: [READ] appended

Patient later revokes Dr. Santos (left that clinic)
  → revoke_access(patient, dr_santos)
  → Access(patient, dr_santos) = false
  → AuditLog: [REVOKE] appended
```

**Demo time: < 2 minutes** — upload one hash, grant access, read from a second wallet, show audit log.

---

## Vision and Purpose

MedChain QC addresses one of Southeast Asia's most tangible healthcare inefficiencies: **the lack of patient-owned, portable medical records**. Filipinos visit an average of 3–4 different health facilities per year. Without a unified record layer, duplicate diagnostics waste billions in regional healthcare spend annually.

By anchoring permissions to Stellar wallets rather than hospital IT systems, MedChain QC shifts data sovereignty to the patient. This is especially critical for chronic disease management (diabetes, hypertension, kidney disease) where longitudinal records are essential for accurate diagnosis.

The architecture is also composable: a future Soroban contract could gate insurance payouts on verified on-chain record timestamps, or allow anonymized aggregate health data to be contributed to public health research with explicit patient consent.

---

## Why This Wins (Hackathon Criteria)

- **Real users, real pain**: Duplicate lab tests are a documented, measurable problem in Philippine healthcare — not a hypothetical
- **Stellar-native**: Uses `require_auth()`, persistent storage, Soroban events — features only available on Stellar's smart contract layer
- **Demo-able in 2 min**: Upload hash → grant → read → show audit log is a clear, complete narrative
- **Local economy**: Designed for QC/NCR but exportable to all of SEA's fragmented healthcare markets

---

## Prerequisites

```bash
# Rust toolchain with Wasm target
rustup target add wasm32-unknown-unknown

# Soroban CLI
cargo install --locked soroban-cli --version 21.0.0

# Verify
soroban --version
# soroban 21.x.x
```

---

## Build

```bash
# From project root
soroban contract build

# Output: target/wasm32-unknown-unknown/release/medchain_qc.wasm
```

---

## Test

```bash
cargo test
# Runs all 5 tests in src/test.rs

# With logging output
cargo test -- --nocapture
```

---

## Deploy to Testnet

```bash
# 1. Configure Stellar testnet
soroban network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"

# 2. Generate or import a test identity
soroban keys generate --network testnet patient_wallet
soroban keys generate --network testnet doctor_wallet

# Fund via Friendbot
curl "https://friendbot.stellar.org?addr=$(soroban keys address patient_wallet)"

# 3. Deploy the contract
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/medchain_qc.wasm \
  --source patient_wallet \
  --network testnet

# Returns: CONTRACT_ID (copy this for invocations below)
export CONTRACT_ID="<paste_contract_id_here>"
```

---

## Sample CLI Invocations

### Upload a record

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source patient_wallet \
  --network testnet \
  -- upload_record \
  --patient $(soroban keys address patient_wallet) \
  --record_hash aabbccdd11223344aabbccdd11223344aabbccdd11223344aabbccdd11223344 \
  --label "CBC Result 2025-03-15" \
  --issuer $(soroban keys address doctor_wallet)
```

### Grant access to a doctor

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source patient_wallet \
  --network testnet \
  -- grant_access \
  --patient $(soroban keys address patient_wallet) \
  --doctor $(soroban keys address doctor_wallet)
```

### Doctor reads records

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source doctor_wallet \
  --network testnet \
  -- read_records \
  --doctor $(soroban keys address doctor_wallet) \
  --patient $(soroban keys address patient_wallet)
```

### Check access status

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --network testnet \
  -- check_access \
  --patient $(soroban keys address patient_wallet) \
  --doctor $(soroban keys address doctor_wallet)
```

### View full audit log

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --network testnet \
  -- get_audit_log
```

### Revoke access

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source patient_wallet \
  --network testnet \
  -- revoke_access \
  --patient $(soroban keys address patient_wallet) \
  --doctor $(soroban keys address doctor_wallet)
```

---

## Optional Enhancements (Bonus Points)

| Enhancement | Description |
|---|---|
| **AI integration** | LLM summarizes record history and flags missing routine tests (e.g., "No HbA1c in 6 months") |
| **Offline support** | Encrypted records cached locally; sync hashes to chain when connectivity returns |
| **Anchor integration** | GCash / Maya anchor for USDC on/off-ramp — patients pay ₱1 per record verification |
| **DeFi composability** | On-chain record timestamps gate insurance micro-payouts via a second Soroban contract |

---

## Project Structure

```
medchain-qc/
├── Cargo.toml          # Package manifest + Soroban dependencies
├── README.md           # This file
├── src/
│   ├── lib.rs          # Soroban smart contract (upload, grant, revoke, read, audit)
│   └── test.rs         # 5 unit tests
└── frontend/
    └── index.html      # Demo UI (React + Freighter wallet integration)
```

---

## License

MIT © 2025 MedChain QC Contributors
Contract ID : CBRJEQNZWSWO4FYKTWCUUOJY4NIBORCUWRGJFNBVPYR6Q35IZCZQOIKS
Stellar Link : https://stellar.expert/explorer/testnet/contract/CBRJEQNZWSWO4FYKTWCUUOJY4NIBORCUWRGJFNBVPYR6Q35IZCZQOIKS
---

*Built for the Stellar Bootcamp 2026. Reference implementation at [https://github.com/armlynobinguar/Stellar-Bootcamp-2026](https://github.com/armlynobinguar/Stellar-Bootcamp-2026)*
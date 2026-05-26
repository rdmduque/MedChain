/*
 * MedChain QC — Tamper-Proof Medical Record Access on Stellar
 *
 * Patients in Quezon City (and anywhere) own their medical record hashes on-chain.
 * Doctors must be explicitly authorized per patient before reading records.
 * Every access attempt is logged immutably for audit purposes.
 *
 * Core MVP flow:
 *   1. Patient uploads a record hash (SHA-256 of encrypted file stored off-chain)
 *   2. Patient grants/revokes doctor access via their Stellar wallet signature
 *   3. Doctor reads record — access is logged on-chain
 *   4. Any party can verify the audit trail without needing the underlying data
 */

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    Address, Bytes, BytesN, Env, Map, Vec, Symbol, log,
};

// ─── Storage Key Types ────────────────────────────────────────────────────────

/// Distinguishes top-level storage namespaces
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Maps patient Address → Vec<RecordEntry>
    Records(Address),
    /// Maps (patient Address, doctor Address) → bool (is authorized)
    Access(Address, Address),
    /// Global append-only audit log: Vec<AuditEntry>
    AuditLog,
    /// Maps patient Address → admin/owner flag (the patient themselves)
    PatientOwner(Address),
}

// ─── Data Structures ──────────────────────────────────────────────────────────

/// One medical record entry stored per patient
#[contracttype]
#[derive(Clone)]
pub struct RecordEntry {
    /// SHA-256 hash of the encrypted file (stored off-chain on IPFS or similar)
    pub record_hash: BytesN<32>,
    /// Human-readable label, e.g. "CBC 2025-03-15" (max 64 bytes enforced off-chain)
    pub label: Bytes,
    /// Ledger timestamp when the record was uploaded
    pub uploaded_at: u64,
    /// Stellar address of the clinic/lab that produced this record
    pub issuer: Address,
}

/// Immutable audit log entry — written on every access grant, revoke, or read
#[contracttype]
#[derive(Clone)]
pub struct AuditEntry {
    /// What happened: "UPLOAD", "GRANT", "REVOKE", "READ"
    pub action: Symbol,
    /// The patient whose data was affected
    pub patient: Address,
    /// The actor performing the action (patient for UPLOAD/GRANT/REVOKE, doctor for READ)
    pub actor: Address,
    /// Ledger timestamp
    pub timestamp: u64,
    /// Optional record hash for READ/UPLOAD events; zero-bytes for GRANT/REVOKE
    pub record_hash: BytesN<32>,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct MedChainContract;

#[contractimpl]
impl MedChainContract {

    // ── 1. UPLOAD RECORD ──────────────────────────────────────────────────────

    /// Called by a patient (or authorized clinic) to register a new medical record hash.
    /// The actual file is encrypted and stored off-chain; only its hash lives here.
    ///
    /// # Arguments
    /// * `patient`     – must sign the transaction (auth required)
    /// * `record_hash` – SHA-256 of the encrypted file
    /// * `label`       – human-readable description ("CBC Result 2025-01-10")
    /// * `issuer`      – address of the clinic/lab that generated the record
    pub fn upload_record(
        env: Env,
        patient: Address,
        record_hash: BytesN<32>,
        label: Bytes,
        issuer: Address,
    ) {
        // Require the patient's wallet signature — only they can upload their records
        patient.require_auth();

        let timestamp = env.ledger().timestamp();

        let entry = RecordEntry {
            record_hash: record_hash.clone(),
            label,
            uploaded_at: timestamp,
            issuer,
        };

        // Append to patient's record list (or create new list)
        let key = DataKey::Records(patient.clone());
        let mut records: Vec<RecordEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(&env));

        records.push_back(entry);
        env.storage().persistent().set(&key, &records);

        // Append to global audit log
        Self::append_audit(
            &env,
            Symbol::new(&env, "UPLOAD"),
            patient.clone(),
            patient,
            timestamp,
            record_hash,
        );

        log!(&env, "Record uploaded successfully");
    }

    // ── 2. GRANT ACCESS ───────────────────────────────────────────────────────

    /// Patient explicitly authorizes a doctor's Stellar address to view their records.
    /// This replaces fragmented paper consent forms with a cryptographically signed grant.
    ///
    /// # Arguments
    /// * `patient` – must sign; owns the data
    /// * `doctor`  – address being granted read access
    pub fn grant_access(env: Env, patient: Address, doctor: Address) {
        patient.require_auth();

        let key = DataKey::Access(patient.clone(), doctor.clone());
        env.storage().persistent().set(&key, &true);

        // Log the grant for audit trail
        let zero_hash: BytesN<32> = BytesN::from_array(&env, &[0u8; 32]);
        Self::append_audit(
            &env,
            Symbol::new(&env, "GRANT"),
            patient.clone(),
            patient,
            env.ledger().timestamp(),
            zero_hash,
        );

        log!(&env, "Access granted to doctor");
    }

    // ── 3. REVOKE ACCESS ──────────────────────────────────────────────────────

    /// Patient removes a doctor's permission — e.g. after leaving a clinic.
    /// Stellar's fast finality means revocation is effective in ~5 seconds.
    ///
    /// # Arguments
    /// * `patient` – must sign
    /// * `doctor`  – address being revoked
    pub fn revoke_access(env: Env, patient: Address, doctor: Address) {
        patient.require_auth();

        let key = DataKey::Access(patient.clone(), doctor.clone());
        env.storage().persistent().set(&key, &false);

        let zero_hash: BytesN<32> = BytesN::from_array(&env, &[0u8; 32]);
        Self::append_audit(
            &env,
            Symbol::new(&env, "REVOKE"),
            patient.clone(),
            patient,
            env.ledger().timestamp(),
            zero_hash,
        );

        log!(&env, "Access revoked from doctor");
    }

    // ── 4. READ RECORDS ───────────────────────────────────────────────────────

    /// Doctor fetches a patient's record list. Requires active authorization.
    /// Each call is logged so patients can see who accessed their data and when.
    ///
    /// # Arguments
    /// * `doctor`  – must sign; their address is checked against the patient's grant list
    /// * `patient` – whose records to fetch
    ///
    /// # Returns
    /// Vec<RecordEntry> — the patient's full record list (hashes + labels + timestamps)
    pub fn read_records(
        env: Env,
        doctor: Address,
        patient: Address,
    ) -> Vec<RecordEntry> {
        doctor.require_auth();

        // Check authorization — panics with a clear message if not granted
        let access_key = DataKey::Access(patient.clone(), doctor.clone());
        let authorized: bool = env
            .storage()
            .persistent()
            .get(&access_key)
            .unwrap_or(false);

        if !authorized {
            panic!("Doctor is not authorized to access this patient's records");
        }

        // Retrieve records
        let records_key = DataKey::Records(patient.clone());
        let records: Vec<RecordEntry> = env
            .storage()
            .persistent()
            .get(&records_key)
            .unwrap_or(Vec::new(&env));

        // Log every read attempt — patients can audit who looked at their data
        let zero_hash: BytesN<32> = BytesN::from_array(&env, &[0u8; 32]);
        Self::append_audit(
            &env,
            Symbol::new(&env, "READ"),
            patient,
            doctor,
            env.ledger().timestamp(),
            zero_hash,
        );

        records
    }

    // ── 5. CHECK ACCESS ───────────────────────────────────────────────────────

    /// Public read — anyone can check if a doctor-patient access grant is active.
    /// Used by clinic frontends to show the current permission state.
    pub fn check_access(env: Env, patient: Address, doctor: Address) -> bool {
        let key = DataKey::Access(patient, doctor);
        env.storage().persistent().get(&key).unwrap_or(false)
    }

    // ── 6. GET AUDIT LOG ──────────────────────────────────────────────────────

    /// Returns the full immutable audit trail.
    /// In production, filter by patient on the client to keep responses small.
    pub fn get_audit_log(env: Env) -> Vec<AuditEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::AuditLog)
            .unwrap_or(Vec::new(&env))
    }

    // ── 7. GET RECORD COUNT ───────────────────────────────────────────────────

    /// Convenience function: returns how many records a patient has uploaded.
    pub fn record_count(env: Env, patient: Address) -> u32 {
        let key = DataKey::Records(patient);
        let records: Vec<RecordEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(&env));
        records.len()
    }

    // ── INTERNAL: Append to audit log ─────────────────────────────────────────

    fn append_audit(
        env: &Env,
        action: Symbol,
        patient: Address,
        actor: Address,
        timestamp: u64,
        record_hash: BytesN<32>,
    ) {
        let entry = AuditEntry {
            action,
            patient,
            actor,
            timestamp,
            record_hash,
        };

        let mut log_vec: Vec<AuditEntry> = env
            .storage()
            .persistent()
            .get(&DataKey::AuditLog)
            .unwrap_or(Vec::new(env));

        log_vec.push_back(entry);
        env.storage().persistent().set(&DataKey::AuditLog, &log_vec);
    }
}
#[cfg(test)]
mod tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Bytes, BytesN, Env, Symbol,
    };

    use crate::{MedChainContract, MedChainContractClient};

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Creates a deterministic 32-byte hash for testing
    fn make_hash(env: &Env, seed: u8) -> BytesN<32> {
        BytesN::from_array(env, &[seed; 32])
    }

    /// Creates a short label as Bytes
    fn make_label(env: &Env, text: &str) -> Bytes {
        Bytes::from_slice(env, text.as_bytes())
    }

    /// Sets up a fresh environment with a fixed timestamp so tests are deterministic
    fn setup_env() -> Env {
        let env = Env::default();
        env.ledger().set(LedgerInfo {
            timestamp: 1_700_000_000,
            protocol_version: 21,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 1000,
            min_persistent_entry_ttl: 1000,
            max_entry_ttl: 10_000,
        });
        env
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 1 — Happy Path: Full MVP transaction executes end-to-end
    //
    // Simulates: Maria (patient) uploads a CBC result, grants access to
    // Dr. Santos, and Dr. Santos successfully reads the record.
    // ─────────────────────────────────────────────────────────────────────────
    #[test]
    fn test_happy_path_upload_grant_read() {
        let env = setup_env();
        let contract_id = env.register_contract(None, MedChainContract);
        let client = MedChainContractClient::new(&env, &contract_id);

        let maria = Address::generate(&env);
        let dr_santos = Address::generate(&env);
        let clinic = Address::generate(&env);

        let hash = make_hash(&env, 0xAB);
        let label = make_label(&env, "CBC Result 2025-03-15");

        // Maria mocks her wallet auth and uploads her CBC result
        env.mock_all_auths();
        client.upload_record(&maria, &hash, &label, &clinic);

        // Maria grants Dr. Santos access
        client.grant_access(&maria, &dr_santos);

        // Dr. Santos reads the records
        let records = client.read_records(&dr_santos, &maria);

        // Verify the returned record matches what was uploaded
        assert_eq!(records.len(), 1);
        let record = records.get(0).unwrap();
        assert_eq!(record.record_hash, hash);
        assert_eq!(record.issuer, clinic);

        // Verify the audit log captured all three actions
        let audit = client.get_audit_log();
        assert_eq!(audit.len(), 3); // UPLOAD, GRANT, READ

        let actions: Vec<Symbol> = audit.iter().map(|e| e.action.clone()).collect();
        assert!(actions.contains(&Symbol::new(&env, "UPLOAD")));
        assert!(actions.contains(&Symbol::new(&env, "GRANT")));
        assert!(actions.contains(&Symbol::new(&env, "READ")));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 2 — Edge Case: Unauthorized doctor cannot read records
    //
    // Dr. Reyes has never been granted access by Maria.
    // The contract must panic and prevent the read.
    // ─────────────────────────────────────────────────────────────────────────
    #[test]
    #[should_panic(expected = "Doctor is not authorized")]
    fn test_unauthorized_doctor_cannot_read() {
        let env = setup_env();
        let contract_id = env.register_contract(None, MedChainContract);
        let client = MedChainContractClient::new(&env, &contract_id);

        let maria = Address::generate(&env);
        let dr_reyes = Address::generate(&env); // never granted access
        let clinic = Address::generate(&env);

        env.mock_all_auths();
        client.upload_record(&maria, &make_hash(&env, 0x01), &make_label(&env, "HbA1c"), &clinic);

        // This should panic — Dr. Reyes has no authorization
        client.read_records(&dr_reyes, &maria);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 3 — State Verification: Storage reflects correct state after MVP flow
    //
    // After upload + grant, verify:
    //   - record_count is correct
    //   - check_access returns true
    //   - after revoke, check_access returns false
    // ─────────────────────────────────────────────────────────────────────────
    #[test]
    fn test_state_reflects_correct_values_after_operations() {
        let env = setup_env();
        let contract_id = env.register_contract(None, MedChainContract);
        let client = MedChainContractClient::new(&env, &contract_id);

        let maria = Address::generate(&env);
        let dr_santos = Address::generate(&env);
        let clinic = Address::generate(&env);

        env.mock_all_auths();

        // Upload two records
        client.upload_record(&maria, &make_hash(&env, 0x01), &make_label(&env, "HbA1c"), &clinic);
        client.upload_record(&maria, &make_hash(&env, 0x02), &make_label(&env, "Creatinine"), &clinic);

        // Verify record count
        assert_eq!(client.record_count(&maria), 2);

        // Grant access and verify
        client.grant_access(&maria, &dr_santos);
        assert!(client.check_access(&maria, &dr_santos));

        // Revoke access and verify state flips to false
        client.revoke_access(&maria, &dr_santos);
        assert!(!client.check_access(&maria, &dr_santos));

        // Audit log should now have: UPLOAD, UPLOAD, GRANT, REVOKE = 4 entries
        assert_eq!(client.get_audit_log().len(), 4);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 4 — Access restored after re-grant following revoke
    //
    // Ensures the permission system is togglable, not one-way.
    // Real-world: Maria leaves Clinic A, goes to Clinic B, later returns to A.
    // ─────────────────────────────────────────────────────────────────────────
    #[test]
    fn test_regrant_after_revoke_restores_access() {
        let env = setup_env();
        let contract_id = env.register_contract(None, MedChainContract);
        let client = MedChainContractClient::new(&env, &contract_id);

        let maria = Address::generate(&env);
        let dr_santos = Address::generate(&env);
        let clinic = Address::generate(&env);

        env.mock_all_auths();

        client.upload_record(&maria, &make_hash(&env, 0xCC), &make_label(&env, "Urinalysis"), &clinic);

        // Grant → revoke → re-grant
        client.grant_access(&maria, &dr_santos);
        client.revoke_access(&maria, &dr_santos);
        client.grant_access(&maria, &dr_santos);

        // Should be able to read again
        let records = client.read_records(&dr_santos, &maria);
        assert_eq!(records.len(), 1);

        // Access should be true again
        assert!(client.check_access(&maria, &dr_santos));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 5 — Multiple patients are isolated from each other
    //
    // Maria's records are not visible to Dr. Reyes who only has
    // authorization from Pedro (a different patient).
    // ─────────────────────────────────────────────────────────────────────────
    #[test]
    #[should_panic(expected = "Doctor is not authorized")]
    fn test_patient_records_are_isolated() {
        let env = setup_env();
        let contract_id = env.register_contract(None, MedChainContract);
        let client = MedChainContractClient::new(&env, &contract_id);

        let maria = Address::generate(&env);
        let pedro = Address::generate(&env);
        let dr_reyes = Address::generate(&env);
        let clinic = Address::generate(&env);

        env.mock_all_auths();

        // Both patients upload records
        client.upload_record(&maria, &make_hash(&env, 0xAA), &make_label(&env, "Maria CBC"), &clinic);
        client.upload_record(&pedro, &make_hash(&env, 0xBB), &make_label(&env, "Pedro CBC"), &clinic);

        // Dr. Reyes gets access to Pedro's records only
        client.grant_access(&pedro, &dr_reyes);

        // Dr. Reyes tries to read Maria's records — must panic
        client.read_records(&dr_reyes, &maria);
    }
}
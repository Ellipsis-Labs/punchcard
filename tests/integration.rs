use borsh::BorshSerialize;
use litesvm::LiteSVM;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::Transaction,
};

const PROGRAM_ID: Pubkey = solana_sdk::pubkey!("pcWKVSdcdDUKabPz4pVfaQ2jMod1kWv3LqeQivjKXiF");

#[derive(BorshSerialize)]
enum PunchcardInstruction {
    Create { capacity: u64 },
    Claim { indices: Vec<u64> },
}

fn create_ix(payer: &Pubkey, punchcard: &Pubkey, capacity: u64) -> Instruction {
    Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(*punchcard, true),
            AccountMeta::new_readonly(Pubkey::new_from_array(pinocchio_system::ID), false),
        ],
        data: borsh::to_vec(&PunchcardInstruction::Create { capacity }).unwrap(),
    }
}

fn claim_ix(authority: &Pubkey, punchcard: &Pubkey, indices: Vec<u64>) -> Instruction {
    Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new(*punchcard, false),
        ],
        data: borsh::to_vec(&PunchcardInstruction::Claim { indices }).unwrap(),
    }
}

fn read_punchcard(svm: &LiteSVM, punchcard: &Pubkey) -> Option<(Pubkey, u64, u64, Vec<u8>)> {
    let account = svm.get_account(punchcard)?;
    let data = &account.data;
    if data.len() < 48 {
        return None;
    }

    let authority = Pubkey::try_from(&data[0..32]).unwrap();
    let capacity = u64::from_le_bytes(data[32..40].try_into().unwrap());
    let claimed = u64::from_le_bytes(data[40..48].try_into().unwrap());
    let bits = data[48..].to_vec();

    Some((authority, capacity, claimed, bits))
}

fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(PROGRAM_ID, "target/deploy/punchcard.so")
        .expect("Run `cargo build-sbf` first");
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    (svm, payer)
}

#[test]
fn test_create_punchcard() {
    let (mut svm, payer) = setup();
    let punchcard = Keypair::new();

    let tx = Transaction::new_signed_with_payer(
        &[create_ix(&payer.pubkey(), &punchcard.pubkey(), 16)],
        Some(&payer.pubkey()),
        &[&payer, &punchcard],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let (authority, capacity, claimed, bits) = read_punchcard(&svm, &punchcard.pubkey()).unwrap();
    assert_eq!(authority, payer.pubkey());
    assert_eq!(capacity, 16);
    assert_eq!(claimed, 0);
    assert_eq!(bits.len(), 2);
    assert!(bits.iter().all(|&b| b == 0));
}

#[test]
fn test_claim_single_index() {
    let (mut svm, payer) = setup();
    let punchcard = Keypair::new();

    let tx = Transaction::new_signed_with_payer(
        &[create_ix(&payer.pubkey(), &punchcard.pubkey(), 16)],
        Some(&payer.pubkey()),
        &[&payer, &punchcard],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[claim_ix(&payer.pubkey(), &punchcard.pubkey(), vec![5])],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let (_, _, claimed, bits) = read_punchcard(&svm, &punchcard.pubkey()).unwrap();
    assert_eq!(claimed, 1);
    assert_eq!(bits[0] & (1 << 5), 1 << 5);
}

#[test]
fn test_claim_multiple_indices() {
    let (mut svm, payer) = setup();
    let punchcard = Keypair::new();

    let tx = Transaction::new_signed_with_payer(
        &[create_ix(&payer.pubkey(), &punchcard.pubkey(), 16)],
        Some(&payer.pubkey()),
        &[&payer, &punchcard],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[claim_ix(
            &payer.pubkey(),
            &punchcard.pubkey(),
            vec![0, 3, 7, 12],
        )],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let (_, _, claimed, bits) = read_punchcard(&svm, &punchcard.pubkey()).unwrap();
    assert_eq!(claimed, 4);
    assert_eq!(bits[0] & (1 << 0), 1 << 0);
    assert_eq!(bits[0] & (1 << 3), 1 << 3);
    assert_eq!(bits[0] & (1 << 7), 1 << 7);
    assert_eq!(bits[1] & (1 << 4), 1 << 4); // index 12 = byte 1, bit 4
}

#[test]
fn test_claim_already_claimed_fails() {
    let (mut svm, payer) = setup();
    let punchcard = Keypair::new();

    let tx = Transaction::new_signed_with_payer(
        &[create_ix(&payer.pubkey(), &punchcard.pubkey(), 16)],
        Some(&payer.pubkey()),
        &[&payer, &punchcard],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[claim_ix(&payer.pubkey(), &punchcard.pubkey(), vec![5])],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[claim_ix(&payer.pubkey(), &punchcard.pubkey(), vec![5])],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    assert!(svm.send_transaction(tx).is_err());
}

#[test]
fn test_claim_out_of_bounds_fails() {
    let (mut svm, payer) = setup();
    let punchcard = Keypair::new();

    let tx = Transaction::new_signed_with_payer(
        &[create_ix(&payer.pubkey(), &punchcard.pubkey(), 16)],
        Some(&payer.pubkey()),
        &[&payer, &punchcard],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[claim_ix(&payer.pubkey(), &punchcard.pubkey(), vec![16])],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    assert!(svm.send_transaction(tx).is_err());
}

#[test]
fn test_claim_wrong_authority_fails() {
    let (mut svm, payer) = setup();
    let punchcard = Keypair::new();
    let wrong = Keypair::new();
    svm.airdrop(&wrong.pubkey(), 1_000_000_000).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[create_ix(&payer.pubkey(), &punchcard.pubkey(), 16)],
        Some(&payer.pubkey()),
        &[&payer, &punchcard],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[claim_ix(&wrong.pubkey(), &punchcard.pubkey(), vec![5])],
        Some(&wrong.pubkey()),
        &[&wrong],
        svm.latest_blockhash(),
    );
    assert!(svm.send_transaction(tx).is_err());
}

#[test]
fn test_claim_all_closes_account() {
    let (mut svm, payer) = setup();
    let punchcard = Keypair::new();

    let tx = Transaction::new_signed_with_payer(
        &[create_ix(&payer.pubkey(), &punchcard.pubkey(), 4)],
        Some(&payer.pubkey()),
        &[&payer, &punchcard],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    let balance_before = svm.get_account(&payer.pubkey()).unwrap().lamports;

    let tx = Transaction::new_signed_with_payer(
        &[claim_ix(
            &payer.pubkey(),
            &punchcard.pubkey(),
            vec![0, 1, 2, 3],
        )],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx).unwrap();

    // Account should be closed
    match svm.get_account(&punchcard.pubkey()) {
        None => {}
        Some(acc) => {
            assert!(acc.lamports == 0 || acc.owner == Pubkey::new_from_array(pinocchio_system::ID))
        }
    }

    // Rent returned
    let balance_after = svm.get_account(&payer.pubkey()).unwrap().lamports;
    assert!(balance_after > balance_before);
}

#[test]
fn test_various_capacities() {
    let (mut svm, payer) = setup();

    for capacity in [1, 7, 8, 9, 15, 16, 17, 64, 100] {
        let punchcard = Keypair::new();
        let tx = Transaction::new_signed_with_payer(
            &[create_ix(&payer.pubkey(), &punchcard.pubkey(), capacity)],
            Some(&payer.pubkey()),
            &[&payer, &punchcard],
            svm.latest_blockhash(),
        );
        svm.send_transaction(tx).unwrap();

        let (_, cap, claimed, bits) = read_punchcard(&svm, &punchcard.pubkey()).unwrap();
        assert_eq!(cap, capacity);
        assert_eq!(claimed, 0);
        assert_eq!(bits.len(), ((capacity + 7) / 8) as usize);
    }
}

use solana_hash::Hash;
use solana_sdk::{
    instruction::Instruction, message::AddressLookupTableAccount, native_token::sol_str_to_lamports, pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::VersionedTransaction,
};
use solana_system_interface::instruction::transfer;
use std::sync::Arc;

use super::{
    compute_budget_manager::compute_budget_instructions,
    nonce_manager::{add_nonce_instruction, get_transaction_blockhash},
};
use crate::{
    common::{nonce_cache::DurableNonceInfo, SolanaRpcClient},
    constants::swqos::NODE1_TIP_ACCOUNTS,
    trading::{MiddlewareManager, core::transaction_pool::{acquire_builder, release_builder}},
};

/// Build standard RPC transaction
pub fn build_transaction(
    payer: Arc<Keypair>,
    unit_limit: u32,
    unit_price: u64,
    business_instructions: Vec<Instruction>,
    address_lookup_table_account: Option<AddressLookupTableAccount>,
    recent_blockhash: Option<Hash>,
    with_tip: bool,
    tip_account: &Pubkey,
    tip_amount: f64,
) -> Result<VersionedTransaction, anyhow::Error> {
    let mut instructions = Vec::with_capacity(business_instructions.len() + 5);

    // Add compute budget instructions
    instructions.extend(compute_budget_instructions(
        unit_price,
        unit_limit,
    ));

    // Add business instructions
    instructions.extend(business_instructions);

    // Add tip transfer instruction
    if with_tip && tip_amount > 0.0 {
        instructions.push(transfer(
            &payer.pubkey(),
            tip_account,
            sol_str_to_lamports(tip_amount.to_string().as_str()).unwrap_or(0),
        ));
    }

    // Build transaction
    build_versioned_transaction(
        payer,
        instructions,
        address_lookup_table_account,
        recent_blockhash.unwrap(),
    )
}

/// Low-level function for building versioned transactions
fn build_versioned_transaction(
    payer: Arc<Keypair>,
    instructions: Vec<Instruction>,
    address_lookup_table_account: Option<AddressLookupTableAccount>,
    blockhash: Hash,
) -> Result<VersionedTransaction, anyhow::Error> {
    // 使用预分配的交易构建器以降低延迟
    let mut builder = acquire_builder();

    let versioned_msg = builder.build_zero_alloc(
        &payer.pubkey(),
        &instructions,
        address_lookup_table_account,
        blockhash,
    );

    let msg_bytes = versioned_msg.serialize();
    let signature = payer.try_sign_message(&msg_bytes).expect("sign failed");
    let tx = VersionedTransaction { signatures: vec![signature], message: versioned_msg };

    // 归还构建器到池
    release_builder(builder);

    Ok(tx)
}

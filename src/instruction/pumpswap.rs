use crate::{
    constants::trade::trade::DEFAULT_SLIPPAGE,
    instruction::utils::pumpswap::{
        accounts, fee_recipient_ata, get_mayhem_fee_recipient_random, get_pool_v2_pda,
        get_protocol_extra_fee_recipient_random, get_user_volume_accumulator_pda,
        get_user_volume_accumulator_quote_ata, get_user_volume_accumulator_wsol_ata,
        BUY_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_DISCRIMINATOR, SELL_DISCRIMINATOR,
    },
    trading::{
        common::wsol_manager,
        core::{
            params::{PumpSwapParams, SwapParams},
            traits::InstructionBuilder,
        },
    },
    utils::calc::pumpswap::{buy_quote_input_internal, sell_base_input_internal},
};
use anyhow::{anyhow, Result};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signer::Signer,
};
use crate::instruction::utils::pumpfun::global_constants::PROTOCOL_EXTRA_FEE_RECIPIENTS;

/// Instruction builder for PumpSwap protocol
pub struct PumpSwapInstructionBuilder;

#[async_trait::async_trait]
impl InstructionBuilder for PumpSwapInstructionBuilder {
    async fn build_buy_instructions(&self, params: &SwapParams) -> Result<Vec<Instruction>> {
        // ========================================
        // Parameter validation and basic data preparation
        // ========================================
        let protocol_params = params
            .protocol_params
            .as_any()
            .downcast_ref::<PumpSwapParams>()
            .ok_or_else(|| anyhow!("Invalid protocol params for PumpSwap"))?;

        if params.input_amount.unwrap_or(0) == 0 {
            return Err(anyhow!("Amount cannot be zero"));
        }

        let pool = protocol_params.pool;
        let base_mint = protocol_params.base_mint;
        let quote_mint = protocol_params.quote_mint;
        let params_coin_creator_vault_ata = protocol_params.coin_creator_vault_ata;
        let params_coin_creator_vault_authority = protocol_params.coin_creator_vault_authority;
        let create_wsol_ata = params.create_input_mint_ata;
        let close_wsol_ata = params.close_input_mint_ata;
        let base_token_program = protocol_params.base_token_program;
        let quote_token_program = protocol_params.quote_token_program;
        let pool_base_token_account = protocol_params.pool_base_token_account;
        let pool_quote_token_account = protocol_params.pool_quote_token_account;

        // ========================================
        // Trade calculation and account address preparation
        // ========================================
        let quote_is_wsol_or_usdc = protocol_params.quote_is_wsol_or_usdc;

        // ========================================
        // Build instructions
        // ========================================
        let mut instructions = Vec::with_capacity(6);

        let input_amount = params.input_amount.unwrap_or(0);
        if create_wsol_ata {
            instructions
                .extend(crate::trading::common::handle_wsol(&params.payer.pubkey(), input_amount));
        }

        if params.create_output_mint_ata {
            instructions.extend(
                crate::common::fast_fn::create_associated_token_account_idempotent_fast_use_seed(
                    &params.payer.pubkey(),
                    &params.payer.pubkey(),
                    if quote_is_wsol_or_usdc { &base_mint } else { &quote_mint },
                    if quote_is_wsol_or_usdc { &base_token_program } else { &quote_token_program },
                    params.open_seed_optimize,
                ),
            );
        }

        if let Some(mut cached_ix) = params.ix.clone() {
            patch_pumpswap_trade_ix_data(
                &mut cached_ix,
                input_amount,
                protocol_params.min_output_amount,
                quote_is_wsol_or_usdc,
                params_coin_creator_vault_ata,
                params_coin_creator_vault_authority,
                true,
            )?;
            instructions.push(cached_ix);
            if close_wsol_ata {
                instructions.extend(crate::trading::common::close_wsol(&params.payer.pubkey()));
            }
            return Ok(instructions);
        }

        let user_base_token_account =
            crate::common::fast_fn::get_associated_token_address_with_program_id_fast_use_seed(
                &params.payer.pubkey(),
                &base_mint,
                &base_token_program,
                params.open_seed_optimize,
            );
        let user_quote_token_account =
            crate::common::fast_fn::get_associated_token_address_with_program_id_fast_use_seed(
                &params.payer.pubkey(),
                &quote_mint,
                &quote_token_program,
                params.open_seed_optimize,
            );

        // Determine fee recipient based on mayhem mode (pump-public-docs: 10th = Mayhem fee recipient, 11th = WSOL ATA of Mayhem; use any one randomly)
        let is_mayhem_mode = protocol_params.is_mayhem_mode;
        let (fee_recipient, fee_recipient_meta) = if is_mayhem_mode {
            get_mayhem_fee_recipient_random()
        } else {
            (accounts::FEE_RECIPIENT, accounts::FEE_RECIPIENT_META)
        };
        let fee_recipient_ata = if is_mayhem_mode {
            fee_recipient_ata(fee_recipient, crate::constants::WSOL_TOKEN_ACCOUNT)
        } else {
            fee_recipient_ata(fee_recipient, quote_mint)
        };

        // Create buy instruction
        let mut accounts = Vec::with_capacity(28);
        accounts.extend([
            AccountMeta::new(pool, false),                          // pool_id
            AccountMeta::new(params.payer.pubkey(), true),          // user (signer)
            accounts::GLOBAL_ACCOUNT_META,                          // global (readonly)
            AccountMeta::new_readonly(base_mint, false),            // base_mint (readonly)
            AccountMeta::new_readonly(quote_mint, false),           // quote_mint (readonly)
            AccountMeta::new(user_base_token_account, false),       // user_base_token_account
            AccountMeta::new(user_quote_token_account, false),      // user_quote_token_account
            AccountMeta::new(pool_base_token_account, false),       // pool_base_token_account
            AccountMeta::new(pool_quote_token_account, false),      // pool_quote_token_account
            fee_recipient_meta,                                     // fee_recipient (readonly)
            AccountMeta::new(fee_recipient_ata, false),             // fee_recipient_ata
            AccountMeta::new_readonly(base_token_program, false),   // TOKEN_PROGRAM_ID (readonly)
            AccountMeta::new_readonly(quote_token_program, false), // TOKEN_PROGRAM_ID (readonly, duplicated as in JS)
            crate::constants::SYSTEM_PROGRAM_META,                 // System Program (readonly)
            accounts::ASSOCIATED_TOKEN_PROGRAM_META, // ASSOCIATED_TOKEN_PROGRAM_ID (readonly)
            accounts::EVENT_AUTHORITY_META,          // event_authority (readonly)
            accounts::AMM_PROGRAM_META,              // PUMP_AMM_PROGRAM_ID (readonly)
            AccountMeta::new(params_coin_creator_vault_ata, false), // coin_creator_vault_ata
            AccountMeta::new_readonly(params_coin_creator_vault_authority, false), // coin_creator_vault_authority (readonly)
        ]);
        if quote_is_wsol_or_usdc {
            accounts.push(accounts::GLOBAL_VOLUME_ACCUMULATOR_META);
            let uva = get_user_volume_accumulator_pda(&params.payer.pubkey())
                .ok_or_else(|| anyhow!("user_volume_accumulator PDA derivation failed"))?;
            accounts.push(AccountMeta::new(uva, false));
        }
        accounts.push(accounts::FEE_CONFIG_META);
        accounts.push(accounts::FEE_PROGRAM_META);
        // Cashback: remaining_accounts[0] = WSOL ATA of UserVolumeAccumulator (after named accounts per IDL)
        if protocol_params.is_cashback_coin {
            if let Some(wsol_ata) = get_user_volume_accumulator_wsol_ata(&params.payer.pubkey()) {
                accounts.push(AccountMeta::new(wsol_ata, false));
            }
        }
        // `pool-v2` only when coin_creator ≠ default (@pump-fun/pump-swap-sdk remainingAccounts)；
        // 否则多出的一格会把 buyback pubkey 错位，触发 BuybackFeeRecipientNotAuthorized（6053）。
        if protocol_params.coin_creator != Pubkey::default() {
            let pool_v2 = get_pool_v2_pda(&base_mint).ok_or_else(|| {
                anyhow!("pool_v2 PDA derivation failed for base_mint {}", base_mint)
            })?;
            accounts.push(AccountMeta::new_readonly(pool_v2, false));
        }
        // Trailing accounts: GlobalConfig.buyback_fee_recipients 中任 pubkey + quote ATA（与 pump-swap-sdk 静态池对齐；轮换时需查链上）。
        let protocol_extra = PROTOCOL_EXTRA_FEE_RECIPIENTS[0];
        accounts.push(AccountMeta::new_readonly(protocol_extra, false));
        accounts.push(AccountMeta::new(
            crate::instruction::utils::pumpswap::fee_recipient_ata(protocol_extra, quote_mint),
            false,
        ));

        // Create instruction data（buy/buy_exact_quote_in 第三参数 track_volume: OptionBool，仅代币支持返现时传 Some(true)；sell 仅两参数）
        let track_volume = if protocol_params.is_cashback_coin { [1u8, 1u8] } else { [1u8, 0u8] }; // Some(true) / Some(false)
        let data: Vec<u8> = if quote_is_wsol_or_usdc {
            let mut buf = [0u8; 26];
            buf[..8].copy_from_slice(&BUY_EXACT_QUOTE_IN_DISCRIMINATOR);
            buf[8..16].copy_from_slice(&input_amount.to_le_bytes());
            buf[16..24].copy_from_slice(&protocol_params.min_output_amount.to_le_bytes());
            buf[24..26].copy_from_slice(&track_volume);
            buf.to_vec()
        } else {
            let mut buf = [0u8; 24];
            buf[..8].copy_from_slice(&SELL_DISCRIMINATOR);
            buf[8..16].copy_from_slice(&input_amount.to_le_bytes());
            buf[16..24].copy_from_slice(&protocol_params.min_output_amount.to_le_bytes());
            buf.to_vec()
        };

        let trade_ix = Instruction { program_id: accounts::AMM_PROGRAM, accounts, data };
        crate::common::fast_fn::cache_pool_trade_instruction(pool, true, trade_ix.clone());
        instructions.push(trade_ix);
        if close_wsol_ata {
            // Close wSOL ATA account, reclaim rent
            instructions.extend(crate::trading::common::close_wsol(&params.payer.pubkey()));
        }
        Ok(instructions)
    }

    async fn build_sell_instructions(&self, params: &SwapParams) -> Result<Vec<Instruction>> {
        // ========================================
        // Parameter validation and basic data preparation
        // ========================================
        let protocol_params = params
            .protocol_params
            .as_any()
            .downcast_ref::<PumpSwapParams>()
            .ok_or_else(|| anyhow!("Invalid protocol params for PumpSwap"))?;

        let pool = protocol_params.pool;
        let base_mint = protocol_params.base_mint;
        let quote_mint = protocol_params.quote_mint;
        let pool_base_token_account = protocol_params.pool_base_token_account;
        let pool_quote_token_account = protocol_params.pool_quote_token_account;
        let params_coin_creator_vault_ata = protocol_params.coin_creator_vault_ata;
        let params_coin_creator_vault_authority = protocol_params.coin_creator_vault_authority;
        let create_wsol_ata = params.create_output_mint_ata;
        let close_wsol_ata = params.close_output_mint_ata;
        let base_token_program = protocol_params.base_token_program;
        let quote_token_program = protocol_params.quote_token_program;

        let input_amount = params.input_amount.unwrap_or(0);
        if input_amount == 0 {
            return Err(anyhow!("Token amount is zero"));
        }

        // ========================================
        // Trade calculation and account address preparation
        // ========================================
        let quote_is_wsol_or_usdc = protocol_params.quote_is_wsol_or_usdc;

        // ========================================
        // Build instructions
        // ========================================
        let mut instructions = Vec::with_capacity(3);

        if create_wsol_ata {
            instructions.extend(wsol_manager::create_wsol_ata(&params.payer.pubkey()));
        }

        if let Some(mut cached_ix) = params.ix.clone() {
            patch_pumpswap_trade_ix_data(
                &mut cached_ix,
                input_amount,
                protocol_params.min_output_amount,
                quote_is_wsol_or_usdc,
                params_coin_creator_vault_ata,
                params_coin_creator_vault_authority,
                false,
            )?;
            instructions.push(cached_ix);

            if close_wsol_ata {
                instructions.extend(crate::trading::common::close_wsol(&params.payer.pubkey()));
            }
            if params.close_input_mint_ata {
                let user_base_token_account =
                    crate::common::fast_fn::get_associated_token_address_with_program_id_fast_use_seed(
                        &params.payer.pubkey(),
                        &base_mint,
                        &base_token_program,
                        params.open_seed_optimize,
                    );
                let user_quote_token_account =
                    crate::common::fast_fn::get_associated_token_address_with_program_id_fast_use_seed(
                        &params.payer.pubkey(),
                        &quote_mint,
                        &quote_token_program,
                        params.open_seed_optimize,
                    );
                instructions.push(crate::common::spl_token::close_account(
                    if quote_is_wsol_or_usdc { &base_token_program } else { &quote_token_program },
                    if quote_is_wsol_or_usdc {
                        &user_base_token_account
                    } else {
                        &user_quote_token_account
                    },
                    &params.payer.pubkey(),
                    &params.payer.pubkey(),
                    &[&params.payer.pubkey()],
                )?);
            }
            return Ok(instructions);
        }

        // Determine fee recipient based on mayhem mode (pump-public-docs: 10th = Mayhem fee recipient, 11th = WSOL ATA of Mayhem; use any one randomly)
        let is_mayhem_mode = protocol_params.is_mayhem_mode;
        let (fee_recipient, fee_recipient_meta) = if is_mayhem_mode {
            get_mayhem_fee_recipient_random()
        } else {
            (accounts::FEE_RECIPIENT, accounts::FEE_RECIPIENT_META)
        };
        let fee_recipient_ata = if is_mayhem_mode {
            fee_recipient_ata(fee_recipient, crate::constants::WSOL_TOKEN_ACCOUNT)
        } else {
            fee_recipient_ata(fee_recipient, quote_mint)
        };

        let user_base_token_account =
            crate::common::fast_fn::get_associated_token_address_with_program_id_fast_use_seed(
                &params.payer.pubkey(),
                &base_mint,
                &base_token_program,
                params.open_seed_optimize,
            );
        let user_quote_token_account =
            crate::common::fast_fn::get_associated_token_address_with_program_id_fast_use_seed(
                &params.payer.pubkey(),
                &quote_mint,
                &quote_token_program,
                params.open_seed_optimize,
            );

        // Create sell instruction
        let mut accounts = Vec::with_capacity(28);
        accounts.extend([
            AccountMeta::new(pool, false),                          // pool_id
            AccountMeta::new(params.payer.pubkey(), true),          // user (signer)
            accounts::GLOBAL_ACCOUNT_META,                          // global (readonly)
            AccountMeta::new_readonly(base_mint, false),            // mint (readonly)
            AccountMeta::new_readonly(quote_mint, false),           // WSOL_TOKEN_ACCOUNT (readonly)
            AccountMeta::new(user_base_token_account, false),       // user_base_token_account
            AccountMeta::new(user_quote_token_account, false),      // user_quote_token_account
            AccountMeta::new(pool_base_token_account, false),       // pool_base_token_account
            AccountMeta::new(pool_quote_token_account, false),      // pool_quote_token_account
            fee_recipient_meta,                                     // fee_recipient (readonly)
            AccountMeta::new(fee_recipient_ata, false),             // fee_recipient_ata
            AccountMeta::new_readonly(base_token_program, false),   // TOKEN_PROGRAM_ID (readonly)
            AccountMeta::new_readonly(quote_token_program, false), // TOKEN_PROGRAM_ID (readonly, duplicated as in JS)
            crate::constants::SYSTEM_PROGRAM_META,                 // System Program (readonly)
            accounts::ASSOCIATED_TOKEN_PROGRAM_META, // ASSOCIATED_TOKEN_PROGRAM_ID (readonly)
            accounts::EVENT_AUTHORITY_META,          // event_authority (readonly)
            accounts::AMM_PROGRAM_META,              // PUMP_AMM_PROGRAM_ID (readonly)
            AccountMeta::new(params_coin_creator_vault_ata, false), // coin_creator_vault_ata
            AccountMeta::new_readonly(params_coin_creator_vault_authority, false), // coin_creator_vault_authority (readonly)
        ]);
        if !quote_is_wsol_or_usdc {
            accounts.push(accounts::GLOBAL_VOLUME_ACCUMULATOR_META);
            let uva = get_user_volume_accumulator_pda(&params.payer.pubkey())
                .ok_or_else(|| anyhow!("user_volume_accumulator PDA derivation failed"))?;
            accounts.push(AccountMeta::new(uva, false));
        }
        accounts.push(accounts::FEE_CONFIG_META);
        accounts.push(accounts::FEE_PROGRAM_META);
        // Cashback sell: 官方 remainingAccounts = [accumulator 的 quote_mint ATA, accumulator PDA, poolV2]（用 quote_mint 非固定 WSOL）
        if protocol_params.is_cashback_coin {
            if let (Some(quote_ata), Some(accumulator)) = (
                get_user_volume_accumulator_quote_ata(
                    &params.payer.pubkey(),
                    &quote_mint,
                    &quote_token_program,
                ),
                get_user_volume_accumulator_pda(&params.payer.pubkey()),
            ) {
                accounts.push(AccountMeta::new(quote_ata, false));
                accounts.push(AccountMeta::new(accumulator, false));
            }
        }
        if protocol_params.coin_creator != Pubkey::default() {
            let pool_v2 = get_pool_v2_pda(&base_mint).ok_or_else(|| {
                anyhow!("pool_v2 PDA derivation failed for base_mint {}", base_mint)
            })?;
            accounts.push(AccountMeta::new_readonly(pool_v2, false));
        }
        let protocol_extra = PROTOCOL_EXTRA_FEE_RECIPIENTS[0];
        accounts.push(AccountMeta::new_readonly(protocol_extra, false));
        accounts.push(AccountMeta::new(
            crate::instruction::utils::pumpswap::fee_recipient_ata(protocol_extra, quote_mint),
            false,
        ));

        // Create instruction data
        let mut data = [0u8; 24];
        if quote_is_wsol_or_usdc {
            data[..8].copy_from_slice(&SELL_DISCRIMINATOR);
            // base_amount_in
            data[8..16].copy_from_slice(&input_amount.to_le_bytes());
            // min_quote_amount_out
            data[16..24].copy_from_slice(&protocol_params.min_output_amount.to_le_bytes());
        } else {
            data[..8].copy_from_slice(&BUY_EXACT_QUOTE_IN_DISCRIMINATOR);
            // base_amount_out
            data[8..16].copy_from_slice(&input_amount.to_le_bytes());
            // max_quote_amount_in
            data[16..24].copy_from_slice(&protocol_params.min_output_amount.to_le_bytes());
        }

        let trade_ix =
            Instruction { program_id: accounts::AMM_PROGRAM, accounts, data: data.to_vec() };
        crate::common::fast_fn::cache_pool_trade_instruction(pool, false, trade_ix.clone());
        instructions.push(trade_ix);

        if close_wsol_ata {
            instructions.extend(crate::trading::common::close_wsol(&params.payer.pubkey()));
        }
        if params.close_input_mint_ata {
            instructions.push(crate::common::spl_token::close_account(
                if quote_is_wsol_or_usdc { &base_token_program } else { &quote_token_program },
                if quote_is_wsol_or_usdc {
                    &user_base_token_account
                } else {
                    &user_quote_token_account
                },
                &params.payer.pubkey(),
                &params.payer.pubkey(),
                &[&params.payer.pubkey()],
            )?);
        }
        Ok(instructions)
    }
}

#[inline]
fn patch_pumpswap_trade_ix_data(
    ix: &mut Instruction,
    amount: u64,
    min_output_amount: u64,
    quote_is_wsol_or_usdc: bool,
    params_coin_creator_vault_ata: Pubkey,
    params_coin_creator_vault_authority: Pubkey,
    is_buy: bool,
) -> Result<()> {
    if ix.data.len() != 24 {
        ix.data.resize(24, 0);
    }

    let discriminator = match (is_buy, quote_is_wsol_or_usdc) {
        (true, true) => &BUY_EXACT_QUOTE_IN_DISCRIMINATOR,
        (true, false) => &SELL_DISCRIMINATOR,
        (false, true) => &SELL_DISCRIMINATOR,
        (false, false) => &BUY_EXACT_QUOTE_IN_DISCRIMINATOR,
    };

    ix.data[..8].copy_from_slice(discriminator);
    ix.data[8..16].copy_from_slice(&amount.to_le_bytes());
    ix.data[16..24].copy_from_slice(&min_output_amount.to_le_bytes());
    if ix.accounts.len() <= 18 {
        return Err(anyhow!("cached PumpSwap instruction missing coin creator accounts"));
    }
    ix.accounts[17] = AccountMeta::new(params_coin_creator_vault_ata, false);
    ix.accounts[18] = AccountMeta::new_readonly(params_coin_creator_vault_authority, false);
    Ok(())
}

/// Claim cashback for PumpSwap (AMM). Transfers WSOL from UserVolumeAccumulator's WSOL ATA to user's WSOL ATA.
/// Caller should ensure user's WSOL ATA exists (e.g. create idempotent ATA instruction) before this instruction.
pub fn claim_cashback_pumpswap_instruction(
    payer: &Pubkey,
    quote_mint: Pubkey,
    quote_token_program: Pubkey,
) -> Option<solana_sdk::instruction::Instruction> {
    const CLAIM_CASHBACK_DISCRIMINATOR: [u8; 8] = [37, 58, 35, 126, 190, 53, 228, 197];
    let user_volume_accumulator = get_user_volume_accumulator_pda(payer)?;
    let user_volume_accumulator_wsol_ata = get_user_volume_accumulator_wsol_ata(payer)?;
    let user_wsol_ata = crate::common::fast_fn::get_associated_token_address_with_program_id_fast(
        payer,
        &quote_mint,
        &quote_token_program,
    );
    // IDL order: user, user_volume_accumulator, quote_mint, quote_token_program,
    // user_volume_accumulator_wsol_token_account, user_wsol_token_account, system_program, event_authority, program
    let accounts = vec![
        AccountMeta::new(*payer, true), // user (signer, writable)
        AccountMeta::new(user_volume_accumulator, false), // user_volume_accumulator (writable)
        AccountMeta::new_readonly(quote_mint, false),
        AccountMeta::new_readonly(quote_token_program, false),
        AccountMeta::new(user_volume_accumulator_wsol_ata, false), // writable
        AccountMeta::new(user_wsol_ata, false),                    // writable
        crate::constants::SYSTEM_PROGRAM_META,
        accounts::EVENT_AUTHORITY_META,
        accounts::AMM_PROGRAM_META,
    ];
    Some(solana_sdk::instruction::Instruction::new_with_bytes(
        accounts::AMM_PROGRAM,
        &CLAIM_CASHBACK_DISCRIMINATOR,
        accounts,
    ))
}

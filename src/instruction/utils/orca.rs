use anyhow::anyhow;
use solana_sdk::pubkey::Pubkey;
use sol_common::common::constants::ORCA_PROGRAM_ID;
use sol_common::protocols::orca::types::Whirlpool;
use crate::common::AnyResult;
use crate::common::fast_fn::{get_cached_pda, PdaCacheKey};
use crate::instruction::utils::orca::seeds::ORACLE_SEED;

pub const SWAP_DISCRIMINATOR: &[u8] = &[0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x87, 0xc8];

pub const TICK_ARRAY_SIZE: i32 = 88;

pub mod seeds {
    pub const ORACLE_SEED: &[u8] = b"oracle";
}

pub fn fetch_tick_arrays_or_default(
    whirlpool_address: &Pubkey,
    whirlpool: &Whirlpool,
) -> AnyResult<Vec<Pubkey>> {
    let tick_array_start_index =
        get_tick_array_start_tick_index(whirlpool.tick_current_index, whirlpool.tick_spacing);
    let offset = whirlpool.tick_spacing as i32 * TICK_ARRAY_SIZE;

    let tick_array_indexes = [
        tick_array_start_index,
        tick_array_start_index + offset,
        tick_array_start_index + offset * 2,
        tick_array_start_index - offset,
        tick_array_start_index - offset * 2,
    ];

    let tick_array_addresses: Vec<Pubkey> = tick_array_indexes
        .iter()
        .filter_map(|&x| get_tick_array_address_from_cache(whirlpool_address, x))
        .collect();

    Ok(tick_array_addresses)
}

fn get_tick_array_start_tick_index(tick_index: i32, tick_spacing: u16) -> i32 {
    let tick_spacing_i32 = tick_spacing as i32;
    let tick_array_size_i32 = TICK_ARRAY_SIZE;
    let real_index = tick_index
        .div_euclid(tick_spacing_i32)
        .div_euclid(tick_array_size_i32);
    real_index * tick_spacing_i32 * tick_array_size_i32
}

fn get_tick_array_address_from_cache(
    whirlpool: &Pubkey,
    start_tick_index: i32,
) -> Option<Pubkey> {
    get_cached_pda(
        PdaCacheKey::OrcaTickArrayAddress(*whirlpool, start_tick_index), || {
            let start_tick_index_str = start_tick_index.to_string();
            let seeds = &[
                b"tick_array",
                whirlpool.as_ref(),
                start_tick_index_str.as_bytes(),
            ];

            let program_id = &ORCA_PROGRAM_ID;
            let pda: Option<(Pubkey, u8)> = Pubkey::try_find_program_address(seeds, program_id);
            pda.map(|pubkey| pubkey.0)
        },
    )
}

pub fn get_oracle_pda_from_cache(pool: &Pubkey) -> Option<Pubkey> {
    get_cached_pda(
        PdaCacheKey::OrcaOracle(*pool), || {
            let seeds: &[&[u8]; 2] = &[ORACLE_SEED, pool.as_ref()];
            let program_id = &ORCA_PROGRAM_ID;
            let pda: Option<(Pubkey, u8)> = Pubkey::try_find_program_address(seeds, program_id);
            pda.map(|pubkey| pubkey.0)
        },
    )
}


use crate::instruction::utils::raydium_amm_v4::accounts::{
    SWAP_FEE_DENOMINATOR, SWAP_FEE_NUMERATOR,
};

fn ceil_fee(amount: u64, fee_numerator: u64, fee_denominator: u64) -> u64 {
    let numerator = (amount as u128) * (fee_numerator as u128);
    ((numerator + fee_denominator as u128 - 1) / fee_denominator as u128) as u64
}

/// Parameters for computing swap amounts and fees.
#[derive(Debug, Clone)]
pub struct ComputeSwapParams {
    /// Whether the entire input amount is traded
    pub all_trade: bool,
    /// The input amount for the swap
    pub amount_in: u64,
    /// The expected output amount from the swap
    pub amount_out: u64,
    /// The minimum acceptable output amount (considering slippage_basis_points)
    pub min_amount_out: u64,
    /// The trading fee amount
    pub fee: u64,
}

/// Result of a swap calculation containing all relevant amounts and fees.
#[derive(Debug, Clone)]
pub struct SwapResult {
    /// The new amount in the input vault after the swap
    pub new_input_vault_amount: u64,
    /// The new amount in the output vault after the swap
    pub new_output_vault_amount: u64,
    /// The actual input amount used in the swap
    pub input_amount: u64,
    /// The actual output amount received from the swap
    pub output_amount: u64,
    /// The swap fee charged
    pub swap_fee: u64,
}

/// Performs a swap calculation based on input amount.
///
/// Calculates the output amount and all associated fees when swapping a specific input amount.
///
/// # Arguments
/// * `input_amount` - The amount of input tokens to swap
/// * `input_vault_amount` - Current amount in the input token vault
/// * `output_vault_amount` - Current amount in the output token vault
/// * `swap_fee_numerator` - The swap fee numerator
/// * `swap_fee_denominator` - The swap fee denominator
///
/// # Returns
/// A `SwapResult` containing all swap calculations and fees
fn swap_base_input(
    input_amount: u64,
    input_vault_amount: u64,
    output_vault_amount: u64,
    swap_fee_numerator: u64,
    swap_fee_denominator: u64,
) -> SwapResult {
    let swap_fee = ceil_fee(input_amount, swap_fee_numerator, swap_fee_denominator);
    let input_amount_less_fees = input_amount.saturating_sub(swap_fee);

    let output_amount_swapped = ((output_vault_amount as u128)
        .saturating_mul(input_amount_less_fees as u128)
        / (input_vault_amount as u128).saturating_add(input_amount_less_fees as u128))
        as u64;

    SwapResult {
        new_input_vault_amount: input_vault_amount.saturating_add(input_amount_less_fees),
        new_output_vault_amount: output_vault_amount.saturating_sub(output_amount_swapped),
        input_amount,
        output_amount: output_amount_swapped,
        swap_fee,
    }
}

/// Computes swap parameters including amounts, fees, and slippage protection.
///
/// This function calculates the expected output amount, minimum output amount (with slippage),
/// and trading fees for a given input amount in a Raydium AMM V4 pool.
///
/// # Arguments
/// * `base_reserve` - The current reserve amount of the base token in the pool
/// * `quote_reserve` - The current reserve amount of the quote token in the pool  
/// * `is_base_in` - Whether the input token is the base token (true) or quote token (false)
/// * `amount_in` - The amount of input tokens to swap
/// * `slippage_basis_points` - The acceptable slippage in basis points (e.g., 100 for 1%)
///
/// # Returns
/// A `ComputeSwapParams` struct containing all computed swap parameters
pub fn compute_swap_amount(
    base_reserve: u64,
    quote_reserve: u64,
    is_base_in: bool,
    amount_in: u64,
    slippage_basis_points: u64,
) -> ComputeSwapParams {
    compute_swap_amount_with_fees(
        base_reserve,
        quote_reserve,
        0,
        0,
        is_base_in,
        amount_in,
        SWAP_FEE_NUMERATOR,
        SWAP_FEE_DENOMINATOR,
        slippage_basis_points,
    )
}

/// Computes swap parameters using Raydium AMM V4's no-orderbook base-in formula.
///
/// This matches `raydium-amm`'s `swap_base_in_v2` path:
/// reserves are first reduced by `need_take_pnl_*`, then swap fee is deducted
/// from input using ceiling division, and the constant-product output is
/// calculated from the fee-adjusted input.
pub fn compute_swap_amount_with_fees(
    base_reserve: u64,
    quote_reserve: u64,
    base_need_take_pnl: u64,
    quote_need_take_pnl: u64,
    is_base_in: bool,
    amount_in: u64,
    swap_fee_numerator: u64,
    swap_fee_denominator: u64,
    slippage_basis_points: u64,
) -> ComputeSwapParams {
    let base_reserve = base_reserve.saturating_sub(base_need_take_pnl);
    let quote_reserve = quote_reserve.saturating_sub(quote_need_take_pnl);
    let (input_reserve, output_reserve) =
        if is_base_in { (base_reserve, quote_reserve) } else { (quote_reserve, base_reserve) };

    let swap_result = swap_base_input(
        amount_in,
        input_reserve,
        output_reserve,
        swap_fee_numerator,
        swap_fee_denominator,
    );

    let min_amount_out = ((swap_result.output_amount as f64)
        * (1.0 - (slippage_basis_points as f64) / 10000.0)) as u64;

    let all_trade = swap_result.input_amount == amount_in;

    ComputeSwapParams {
        all_trade,
        amount_in,
        amount_out: swap_result.output_amount,
        min_amount_out,
        fee: swap_result.swap_fee,
    }
}

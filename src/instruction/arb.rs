use solana_client::rpc_response::transaction::Instruction;
use crate::trading::{InstructionBuilder, SwapParams};

pub struct ArbInstructionBuilder;

#[async_trait::async_trait]
impl InstructionBuilder for ArbInstructionBuilder {
    async fn build_buy_instructions(&self, params: &SwapParams) -> anyhow::Result<Vec<Instruction>> {
        Ok(vec![])
    }

    async fn build_sell_instructions(&self, params: &SwapParams) -> anyhow::Result<Vec<Instruction>> {
        Ok(vec![])
    }
}
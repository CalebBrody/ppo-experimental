use burn::{
    nn::{Linear, LinearConfig},
    prelude::*,
};
use rl4burn::{MaskedActorCritic, TransformerBlock, TransformerBlockConfig};

pub const TOKENS: usize = 13;
pub const FEATURES: usize = 256;
pub const STATE: usize = TOKENS * FEATURES;
pub const ACTIONS: usize = 2048;
pub const ACTION_FEATURES: usize = 32;
pub const OBS: usize = STATE + ACTIONS * ACTION_FEATURES;

/// Shared entity transformer, joint-action scorer, and state-value head.
#[derive(Module, Debug)]
pub struct Model<B: Backend> {
    input: Linear<B>,
    blocks: Vec<TransformerBlock<B>>,
    action: Linear<B>,
    policy: Linear<B>,
    value: Linear<B>,
}

impl<B: Backend> Model<B> {
    pub fn new(device: &B::Device) -> Self {
        Self {
            input: LinearConfig::new(FEATURES, 64).init(device),
            blocks: (0..2)
                .map(|_| TransformerBlockConfig::new(64, 4).init(device))
                .collect(),
            action: LinearConfig::new(ACTION_FEATURES, 64).init(device),
            policy: LinearConfig::new(64, 1).init(device),
            value: LinearConfig::new(64, 1).init(device),
        }
    }
}

impl<B: Backend> MaskedActorCritic<B> for Model<B> {
    fn forward(&self, obs: Tensor<B, 2>) -> (Tensor<B, 2>, Tensor<B, 1>) {
        let batch = obs.dims()[0];
        let mut state = self.input.forward(
            obs.clone()
                .slice([0..batch, 0..STATE])
                .reshape([batch, TOKENS, FEATURES]),
        );
        for block in &self.blocks {
            state = block.forward(state, None);
        }
        let state = state.mean_dim(1);
        let value = self.value.forward(state.clone()).reshape([batch]).tanh();
        let actions = obs
            .slice([0..batch, STATE..OBS])
            .reshape([batch, ACTIONS, ACTION_FEATURES]);
        let encoded = (self.action.forward(actions) + state).tanh();
        (
            self.policy.forward(encoded).reshape([batch, ACTIONS]),
            value,
        )
    }
}

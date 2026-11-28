//! The `block_should_freeze` hook: declarations read by the mod API generator.

pub trait Hooks {
    /// Live precipitation is about to turn source water into ice at `pos`.
    /// `from` and `to` are block state IDs. Set `cancel` to leave the water in place.
    /// Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "horizon_client::game::native_hooks::base::GameLevel<'_>::native_block_should_freeze"
    )]
    fn block_should_freeze();
}

//! Platform-free core of the SonicScript desktop app (M1 plan: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`).
//! Rules and prompts come from `spec/` at build time; nothing here touches Windows.

pub mod encode;
pub mod groq;
pub mod normalize;
pub mod pipeline;
pub mod spec;
pub mod vad;

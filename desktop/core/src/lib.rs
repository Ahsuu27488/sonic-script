//! Platform-free core of the SonicScript desktop app (M1 plan: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`).
//! Rules and prompts come from `spec/` at build time; nothing here touches Windows.

pub mod api;
pub mod client;
pub mod deadline;
pub mod encode;
pub mod gemini;
pub mod groq;
pub mod keys;
pub mod normalize;
pub mod pipeline;
pub mod resample;
pub mod spec;
pub mod vad;
pub mod words;

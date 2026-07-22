//! UI modules: the sidebar shell, the status footer, the pages, the CP3
//! editors (settings form, dictionary), and the CP4 panels (history, stats).
//! Every color, size, and spacing value comes from `theme`; modules stay
//! under ~300 lines each (design guardrails §1).

pub mod dictionary;
pub mod footer;
pub mod format;
pub mod history;
pub mod invocations;
pub mod pages;
pub mod settings;
pub mod shell;
pub mod stats;
pub mod widgets;

//! UI modules: the sidebar shell, the status footer, the pages, and the
//! CP3 editors (settings form, dictionary). Every color, size, and spacing
//! value comes from `theme`; modules stay under ~300 lines each (design
//! guardrails §1).

pub mod dictionary;
pub mod footer;
pub mod pages;
pub mod settings;
pub mod shell;
pub mod widgets;

//! Application forms: the pure planning core (`jobbot_domain::form`), the port the use cases
//! drive (`FormFiller`) and the Playwright filler that implements it (`jobbot_browser`), under the
//! one name the rest of the application uses.

pub use jobbot_app::ports::{FillMode, FormFiller};
pub use jobbot_browser::form::BrowserFiller;
pub use jobbot_domain::form::*;

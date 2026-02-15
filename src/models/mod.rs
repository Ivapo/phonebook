pub mod ai_preferences;
pub mod availability;
pub mod booking;
pub mod conversation;
pub mod inbox;
pub mod intent;
pub mod user;

pub use ai_preferences::AiPreferences;
pub use availability::Availability;
pub use booking::{Booking, BookingStatus};
pub use conversation::{Conversation, ConversationData, ConversationMessage, ConversationState, PendingBooking};
pub use inbox::{InboxEvent, InboxThread};
pub use intent::{ExtractedIntent, Intent};
pub use user::User;

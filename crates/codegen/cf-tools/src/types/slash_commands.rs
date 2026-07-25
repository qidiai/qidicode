//! Slash command definitions (stub — originally protobuf-generated).
//! Replaced with plain Rust types for workspace compilation.

/// The tool name for the UPDATE_GOAL slash command.
pub const UPDATE_GOAL_TOOL_NAME: &str = "qidi_build:UpdateGoal";

/// The tool name for the CREATE_TASK slash command.
pub const CREATE_TASK_TOOL_NAME: &str = "qidi_build:CreateTask";

/// The tool name for the COMPLETE_TASK slash command.
pub const COMPLETE_TASK_TOOL_NAME: &str = "qidi_build:CompleteTask";

/// A slash command configuration.
#[derive(Debug, Clone)]
pub struct SlashCommandConfig {
    pub name: String,
    pub description: String,
    pub tool_name: String,
}

impl SlashCommandConfig {
    pub fn new(name: &str, description: &str, tool_name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            tool_name: tool_name.to_string(),
        }
    }
}

/// Image generation tool name.
pub const IMAGE_GEN_TOOL_NAME: &str = "qidi_build:ImageGen";

/// Imagine command name.
pub const IMAGINE_COMMAND_NAME: &str = "/imagine";

/// Imagine instruction text.
pub fn imagine_instruction() -> String { "Generate an image from a text description.".to_string() }

/// Imagine usage message.
pub fn imagine_usage_message() -> String { "Usage: /imagine <description>".to_string() }

/// Image to video tool name.
pub const IMAGE_TO_VIDEO_TOOL_NAME: &str = "qidi_build:ImageToVideo";

/// Imagine video command name.
pub const IMAGINE_VIDEO_COMMAND_NAME: &str = "/imagine-video";

/// Imagine video instruction text.
pub fn imagine_video_instruction() -> String { "Generate a video from an image.".to_string() }

/// Imagine video usage message.
pub fn imagine_video_usage_message() -> String { "Usage: /imagine-video <image_url> <description>".to_string() }

/// Loop scheduler tool name.
pub const SCHEDULER_CREATE_TOOL_NAME: &str = "qidi_build:SchedulerCreate";

pub const LOOP_SCHEDULE_TOOL_NAME: &str = "qidi_build:LoopSchedule";

/// Loop usage message.
pub fn loop_usage_message() -> String { "Usage: /loop <command> <interval>".to_string() }

/// Loop schedule instruction.
pub fn loop_schedule_instruction(_args: &str) -> String { "Schedule a command to run at regular intervals.".to_string() }


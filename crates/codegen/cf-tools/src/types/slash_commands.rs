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
/// Build the model instruction that `/imagine <prompt>` expands into.
///
/// Restored from upstream xai-grok-tools-api (that crate was not carried
/// over at fork time; the stub broke the cf-pager injection-text pins that
/// upstream CI exercises).
pub fn imagine_instruction(args: &str) -> String {
    format!(
        "# /imagine -- generate an image\n\n\
         Generate an image with the image_gen tool using the user's description below.\n\n\
         ## Action\n\
         1. Call image_gen with: prompt (the user's description, kept verbatim -- do not\n\
            paraphrase), n: 1. The tool returns a saved image path.\n\
         2. Show the returned image path to the user.\n\
         3. Do NOT describe or imagine the image inline -- the tool produces it.\n\n\
         ## Input\n\
         {args}"
    )
}

/// Imagine usage message.
pub fn imagine_usage_message() -> String { "Usage: /imagine <description>".to_string() }

/// Image to video tool name.
pub const IMAGE_TO_VIDEO_TOOL_NAME: &str = "qidi_build:ImageToVideo";

/// Imagine video command name.
pub const IMAGINE_VIDEO_COMMAND_NAME: &str = "/imagine-video";

/// Imagine video instruction text.
/// Build the model instruction that `/imagine-video <prompt>` expands into.
///
/// Restored alongside imagine_instruction (same upstream crate debt).
pub fn imagine_video_instruction(args: &str) -> String {
    format!(
        "# /imagine-video -- generate a video\n\n\
         Produce a video from the user's description below.\n\n\
         ## Action\n\
         1. Call image_to_video with: prompt (the user's description, kept verbatim),\n\
            n: 1. The tool returns a saved video path.\n\
         2. Show the returned video path to the user.\n\
         3. Do NOT describe the video inline -- the tool produces it.\n\n\
         If the description implies a scene you cannot produce directly, first generate\n\
         a still with image_gen and animate it with reference_to_video, then continue\n\
         from step 2 with the video tool.\n\n\
         ## Input\n\
         {args}"
    )
}

/// Imagine video usage message.
pub fn imagine_video_usage_message() -> String { "Usage: /imagine-video <image_url> <description>".to_string() }

/// Loop scheduler tool name.
pub const SCHEDULER_CREATE_TOOL_NAME: &str = "qidi_build:SchedulerCreate";

pub const LOOP_SCHEDULE_TOOL_NAME: &str = "qidi_build:LoopSchedule";

/// Loop usage message.
pub fn loop_usage_message() -> String { "Usage: /loop <command> <interval>".to_string() }

/// Build the model instruction that `/loop` expands into for `args`.
///
/// The model, not brittle host parsing, turns the request into the
/// `scheduler_create` interval, accepting every natural phrasing and erroring
/// on bad input rather than silently defaulting. Restored from upstream
/// xai-grok-tools-api (that crate was not carried over at fork time; the stub
/// broke the cf-shell wording pins that upstream CI exercises).
pub fn loop_schedule_instruction(args: &str) -> String {
    format!("
# /loop -- schedule a recurring prompt

Parse the input below into an interval and a prompt, then schedule it with scheduler_create.

## Deriving the interval
Read how often to run from the user's request -- however they phrase it -- and convert it
to a compact `<number><unit>` string, where unit is one of `s` (seconds), `m` (minutes),
`h` (hours), or `d` (days). The interval may appear at the start or end of the request;
extract it and use the remaining text as the prompt.

The minimum interval is 60 seconds; shorter values are raised to 60s, so tell the user if that applies.

If the request contains no interval at all, ask the user how often it should run before
scheduling. Do NOT invent or assume a default interval.

## Action
1. Call scheduler_create with: interval (the compact string you derived), prompt,
   recurring: true, fire_immediately: true. If the interval is unparseable, the tool
   returns an error -- fix the interval string rather than guessing.
2. Confirm: what's scheduled, the cadence, that it auto-expires after 7 days,
   and that they can cancel with scheduler_delete (include the job ID).
3. Do NOT execute the prompt inline. The scheduler will fire it immediately.

## Input
{args}
")
}


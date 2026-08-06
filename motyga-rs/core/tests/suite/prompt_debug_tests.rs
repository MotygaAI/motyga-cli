use std::sync::Arc;

use anyhow::Result;
use motyga_core::build_prompt_input;
use motyga_core::config::ConfigBuilder;
use motyga_core::config::ConfigOverrides;
use motyga_home::MotygaHomeUserInstructionsProvider;
use motyga_protocol::models::ContentItem;
use motyga_protocol::models::ResponseItem;
use motyga_protocol::user_input::UserInput;
use core_test_support::responses::strip_metadata;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const TEST_INSTRUCTIONS: &str = "Global test instructions";

#[tokio::test]
async fn build_prompt_input_includes_context_and_user_message() -> Result<()> {
    let motyga_home = TempDir::new()?;
    let cwd = TempDir::new()?;
    std::fs::write(motyga_home.path().join("AGENTS.md"), TEST_INSTRUCTIONS)?;
    let config = ConfigBuilder::default()
        .motyga_home(motyga_home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(cwd.path().to_path_buf()),
            motyga_self_exe: Some(std::env::current_exe()?),
            ..ConfigOverrides::default()
        })
        .build()
        .await?;
    let user_instructions_provider = Arc::new(MotygaHomeUserInstructionsProvider::new(
        config.motyga_home.clone(),
    ));

    let input = build_prompt_input(
        config,
        vec![UserInput::Text {
            text: "hello from debug prompt".to_string(),
            text_elements: Vec::new(),
        }],
        /*state_db*/ None,
        user_instructions_provider,
    )
    .await?;

    let expected_user_message = ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello from debug prompt".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    assert_eq!(
        input.last().cloned().map(strip_metadata),
        Some(expected_user_message)
    );
    assert!(input.iter().any(|item| {
        let ResponseItem::Message { content, .. } = item else {
            return false;
        };

        content.iter().any(|content_item| {
            let (ContentItem::InputText { text } | ContentItem::OutputText { text }) = content_item
            else {
                return false;
            };
            text.contains(TEST_INSTRUCTIONS)
        })
    }));

    Ok(())
}

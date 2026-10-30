//! Media sub-page — research transcription and visual analysis models.
//!
//! Reads from `kask_bridge::KaskMediaSettings` (the `"kask.media"` section
//! in settings.json) and writes via `kask_string_input` — the same pattern
//! as every other kask settings sub-page. The media MCP server reads these
//! as env vars (`HKASK_MEDIA_STT_MODEL`, `HKASK_MEDIA_VISION_MODEL`,
//! `HKASK_MEDIA_SOUND_ANNOTATION_MODEL`, `HKASK_MEDIA_TAGGING_MODEL`)
//! emitted by `mcp_env::emit_media_env` at server launch. The role models
//! (sound annotation, grounded tagging) render on the Models sub-page per
//! the operator's settings > kask > models ruling.

use super::*;

pub(crate) fn render_media_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let media: kask_bridge::KaskMediaSettings = raw
        .and_then(|c| c.media)
        .map(Into::into)
        .unwrap_or_default();

    let stt_input = kask_string_input(
        "kask-media-stt",
        "STT Model",
        "Provider/model-id (clear to disable transcription)",
        media.stt_model,
        "media",
        "stt_model",
    );
    let vision_input = kask_string_input(
        "kask-media-vision",
        "Vision Model",
        "Provider/model-id (clear to disable visual analysis)",
        media.vision_model,
        "media",
        "vision_model",
    );

    v_flex()
        .id("kask-media-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(Headline::new("Media Server").size(HeadlineSize::XLarge))
        .child(
            Label::new(
                "Curate existing research evidence: ingest recordings and images, \
                 organize assets, annotate observations, and extract source-linked excerpts. \
                 Configure speech-to-text and vision models below. Settings provide defaults; \
                 explicitly clearing a model disables that pipeline with a visible error. \
                 Open the media panel from View > Media or the status bar button.",
            )
            .color(Color::Muted)
            .size(LabelSize::Small),
        )
        .child(div().mt_4())
        .child(v_flex().gap_3().child(stt_input).child(vision_input))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::Fs;

    /// expect: [P7] Only research model controls have a media save path.
    #[test]
    fn media_controls_match_research_save_paths() {
        let page = include_str!("media.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production media page");
        let dispatcher = include_str!("../kask_page.rs");
        for field in ["stt_model", "vision_model"] {
            assert!(page.contains(&format!("\"{field}\",")));
            assert!(dispatcher.contains(&format!("(\"media\", \"{field}\") => {{")));
        }
        for field in ["tts_model", "image_gen_model", "video_model"] {
            assert!(!page.contains(field));
            assert!(!dispatcher.contains(&format!("(\"media\", \"{field}\") => {{")));
        }
    }

    /// expect: [P7] Saving/reloading either research model preserves its value;
    /// an explicit clear stays empty and suppresses its child env injection.
    /// Exercises the real file writer/reload, not rendered input interaction.
    #[gpui::test]
    async fn media_models_persist_reload_and_clear(cx: &mut gpui::TestAppContext) {
        let fs = fs::FakeFs::new(cx.executor());
        fs.create_dir(paths::settings_file().parent().expect("settings directory"))
            .await
            .expect("create fake settings directory");
        fs.insert_file(paths::settings_file(), b"{}".to_vec()).await;
        cx.update(settings::init);
        for value in ["OpenRouter/research-model", ""] {
            let completion = cx.update(|cx| {
                SettingsStore::global(cx).update_settings_file_with_completion(
                    fs.clone(),
                    move |settings, _| {
                        let media = settings
                            .kask
                            .get_or_insert_default()
                            .media
                            .get_or_insert_default();
                        media.stt_model = Some(value.into());
                        media.vision_model = Some(value.into());
                        media.sound_annotation_model = Some(value.into());
                        media.tagging_model = Some(value.into());
                    },
                )
            });
            completion
                .await
                .expect("writer finished")
                .expect("settings saved");
            let saved = fs
                .load(paths::settings_file())
                .await
                .expect("read saved settings");
            cx.update(|cx| {
                let mut reloaded = SettingsStore::new(cx, &settings::default_settings());
                reloaded
                    .set_user_settings(&saved, cx)
                    .expect("reload media settings");
                cx.set_global(reloaded);
                let content = raw_kask_settings(cx).expect("persisted media settings");
                let resolved: kask_bridge::KaskSettings = content.into();
                assert_eq!(resolved.media.stt_model, value);
                assert_eq!(resolved.media.vision_model, value);
                assert_eq!(resolved.media.sound_annotation_model, value);
                assert_eq!(resolved.media.tagging_model, value);
                let env = resolved.mcp_env();
                for key in [
                    "HKASK_MEDIA_STT_MODEL",
                    "HKASK_MEDIA_VISION_MODEL",
                    "HKASK_MEDIA_SOUND_ANNOTATION_MODEL",
                    "HKASK_MEDIA_TAGGING_MODEL",
                ] {
                    assert_eq!(
                        env.get(key).map(String::as_str),
                        (!value.is_empty()).then_some(value)
                    );
                }
                for key in [
                    "HKASK_MEDIA_TTS_MODEL",
                    "HKASK_MEDIA_IMAGE_GEN_MODEL",
                    "HKASK_MEDIA_VIDEO_MODEL",
                ] {
                    assert!(!env.contains_key(key));
                }
            });
        }
    }
}

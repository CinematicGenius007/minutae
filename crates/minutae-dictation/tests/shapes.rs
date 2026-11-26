use minutae_dictation::{
    platform::identity::{AppIdentity, AppIdentityLookup, FakeAppIdentityLookup},
    postprocess::{process, process_with_shape},
    shapes::{
        AppMatcher, AppRule, Capitals, LineBreaks, Shape, ShapePreset, resolve_shape,
        suggested_preset,
    },
};

fn apply(preset: ShapePreset, input: &str) -> String {
    process_with_shape(input, &preset.shape(), &[])
}

#[test]
fn default_matches_legacy_tidy_and_old_toggle_keeps_its_contract() {
    let vocab = ["SwiftUI".to_owned(), "Minutae".to_owned()];
    for input in [
        "",
        "[BLANK_AUDIO]",
        "um so I think we should ship it",
        "hello comma world new paragraph swift ui",
        "go to the sto- store new line new paragraph",
        "wait for minutiae full stop does that work question mark",
        "don't touch iPhone e.g. this one",
        "Hello, comma, world.",
        "she said \"ok\"",
    ] {
        assert_eq!(
            process(input, true, &vocab),
            process_with_shape(input, &Shape::default(), &vocab),
            "{input:?}"
        );
    }
    assert_eq!(
        process("um swift ui comma go new line now", false, &vocab),
        "um SwiftUI, go\nnow"
    );
}

#[test]
fn chat_removes_only_the_expected_intro_and_single_line_period() {
    let cases = [
        (
            "um so sounds good comma I'll I'll ship it after lunch period",
            "sounds good, I'll ship it after lunch",
        ),
        ("So, sounds good.", "sounds good"),
        ("so we can leave now", "so we can leave now"),
        ("so sounds good to me", "so sounds good to me"),
        ("so long and thanks", "so long and thanks"),
        ("so that works", "so that works"),
        ("i think so", "I think so"),
        ("it's so so good", "it's so so good"),
        ("is it ready question mark", "is it ready?"),
        ("wow exclamation point", "wow!"),
        ("well...", "well..."),
        ("he said \"ok.\"", "he said \"ok\""),
        ("first. new line second period", "first.\nsecond."),
        ("first.\nsecond.", "first.\nsecond."),
    ];
    for (input, want) in cases {
        assert_eq!(apply(ShapePreset::ChatMessage, input), want, "{input:?}");
    }
}

#[test]
fn email_and_list_layout_only_use_existing_words() {
    for (preset, input, want) in [
        (
            ShapePreset::Email,
            "hi Priya comma the plan holds for May. thanks",
            "Hi Priya,\n\nThe plan holds for May.\n\nThanks",
        ),
        (
            ShapePreset::Email,
            "dear team comma new paragraph the plan holds. new line kind regards new line",
            "Dear team,\n\nThe plan holds.\nKind regards",
        ),
        (
            ShapePreset::Email,
            "the plan holds thanks to Priya",
            "The plan holds thanks to Priya.",
        ),
        (ShapePreset::Email, "the plan holds", "The plan holds."),
        (
            ShapePreset::Email,
            "the best plan wins",
            "The best plan wins.",
        ),
        (
            ShapePreset::NotesAndLists,
            "shopping colon new line bullet point buy milk new line bullet point buy eggs",
            "Shopping:\n• Buy milk\n• Buy eggs",
        ),
        (
            ShapePreset::NotesAndLists,
            "add a bullet point here",
            "Add a bullet point here.",
        ),
    ] {
        assert_eq!(apply(preset, input), want, "{input:?}");
    }
}

#[test]
fn literal_presets_protect_identifiers_and_shell_syntax() {
    let vocab = [
        "GitHub".to_owned(),
        "SwiftUI".to_owned(),
        "Node.js".to_owned(),
    ];
    let input = "git  status --short; echo 'node js' [swift_ui] ./github\n";
    for preset in [ShapePreset::Verbatim, ShapePreset::CodeAndCommit] {
        let shape = Shape {
            capitals: Capitals::Sentence,
            tidy: true,
            ..preset.shape()
        };
        assert_eq!(
            process_with_shape(input, &shape, &vocab),
            "git  status --short; echo 'node js' [swift_ui] ./github"
        );
        assert_eq!(
            process_with_shape("comma period", &shape, &vocab),
            "comma period"
        );
        assert_eq!(process_with_shape("i", &shape, &vocab), "i");
        assert_eq!(process_with_shape("{}", &shape, &vocab), "{}");
    }
    assert_eq!(
        apply(ShapePreset::Verbatim, "printf hello\r\ngit status"),
        "printf hello git status"
    );
    assert_eq!(
        apply(
            ShapePreset::CodeAndCommit,
            "fn main() {\r\n    print();\r\n}\r\n"
        ),
        "fn main() {\n    print();\n}"
    );
    let code = Shape {
        spoken_punctuation: true,
        ..ShapePreset::CodeAndCommit.shape()
    };
    assert_eq!(
        process_with_shape("fix colon flush audio chunks every 30s", &code, &vocab),
        "fix: flush audio chunks every 30s"
    );
    assert_eq!(
        process_with_shape("let [comma] = sys::fn_name", &code, &vocab),
        "let [comma] = sys::fn_name"
    );
}

#[test]
fn rules_are_independent_and_rewrite_is_inert() {
    let input = "um hello hello comma i agree new line we should ship period";
    for (capitals, want) in [
        (
            Capitals::Sentence,
            "Um hello hello comma I agree\nWe should ship period",
        ),
        (
            Capitals::AsSaid,
            "um hello hello comma i agree\nwe should ship period",
        ),
        (
            Capitals::Relaxed,
            "um hello hello comma I agree\nwe should ship period",
        ),
    ] {
        let shape = Shape {
            capitals,
            tidy: false,
            spoken_punctuation: false,
            ..Shape::default()
        };
        assert_eq!(process_with_shape(input, &shape, &[]), want);
        assert_eq!(
            process_with_shape(
                input,
                &Shape {
                    rewrite_with_local_model: true,
                    ..shape
                },
                &[]
            ),
            want
        );
    }

    let single = Shape {
        line_breaks: LineBreaks::SingleLine,
        ..Shape::default()
    };
    let preserve = Shape {
        line_breaks: LineBreaks::Preserve,
        ..Shape::default()
    };
    assert_eq!(
        process_with_shape("hello\r\nworld new line go", &single, &[]),
        "Hello world new line go."
    );
    assert_eq!(
        process_with_shape("hello\r\nworld new line go", &preserve, &[]),
        "Hello\nWorld new line go."
    );
}

#[test]
fn nothing_emits_a_trailing_line_break() {
    for preset in [
        ShapePreset::Verbatim,
        ShapePreset::Tidy,
        ShapePreset::ChatMessage,
        ShapePreset::Email,
        ShapePreset::NotesAndLists,
        ShapePreset::CodeAndCommit,
    ] {
        for input in [
            "hello new line new paragraph\r\n",
            "hello\n\r  \n\t",
            "hello\u{85}\u{2028}\u{2029}",
            "new line",
            "\n \r\n",
        ] {
            let result = apply(preset, input);
            assert_eq!(result, result.trim_end(), "{preset:?}, {input:?}");
        }
    }
}

#[test]
fn stable_app_identity_suggestions() {
    use AppMatcher::*;
    use ShapePreset::*;
    for (matcher, expected) in [
        (
            MacosBundleId("COM.TINYspeck.slackmacgap".into()),
            Some(ChatMessage),
        ),
        (MacosBundleId("com.apple.mail".into()), Some(Email)),
        (MacosBundleId("com.apple.Notes".into()), Some(NotesAndLists)),
        (MacosBundleId("com.apple.Terminal".into()), Some(Verbatim)),
        (WindowsExecutable("OUTLOOK.EXE".into()), Some(Email)),
        (WindowsExecutable("olk".into()), Some(Email)),
        (WindowsExecutable("Code.exe".into()), Some(CodeAndCommit)),
        (
            WindowsExecutable("WindowsTerminal.exe".into()),
            Some(Verbatim),
        ),
        (X11WindowClass("Obsidian".into()), Some(NotesAndLists)),
        (X11WindowClass("Gnome-terminal".into()), Some(Verbatim)),
        (X11WindowClass("some terminal".into()), None),
        (DisplayName("Slack".into()), Some(ChatMessage)),
        (DisplayName("Slack client".into()), None),
    ] {
        let mut app = AppIdentity::default();
        match &matcher {
            MacosBundleId(id) => app.macos_bundle_id = Some(id.clone()),
            WindowsExecutable(exe) => app.windows_executable = Some(exe.clone()),
            X11WindowClass(class) => app.x11_window_class = Some(class.clone()),
            DisplayName(name) => app.display_name = Some(name.clone()),
        }
        assert!(matcher.matches(&app), "{matcher:?}");
        assert_eq!(suggested_preset(&app), expected, "{app:?}");
    }
}

#[test]
fn selection_precedence_and_unknown_identity() {
    let default = ShapePreset::Email.shape();
    let terminal = AppIdentity {
        windows_executable: Some("WindowsTerminal".into()),
        display_name: Some("Windows Terminal".into()),
        ..AppIdentity::default()
    };
    let fake = FakeAppIdentityLookup(Some(terminal.clone()));
    let rule = AppRule {
        app: AppMatcher::WindowsExecutable("windowsterminal.EXE".into()),
        shape: Shape {
            rewrite_with_local_model: true,
            ..ShapePreset::ChatMessage.shape()
        },
    };
    let override_rule = AppRule {
        shape: default,
        ..rule.clone()
    };
    assert_eq!(
        resolve_shape(fake.frontmost_app().as_ref(), &[], &default),
        ShapePreset::Verbatim.shape()
    );
    assert_eq!(
        resolve_shape(Some(&terminal), &[rule.clone(), override_rule], &default),
        rule.shape
    );
    assert_eq!(
        resolve_shape(
            FakeAppIdentityLookup::default().frontmost_app().as_ref(),
            &[rule],
            &default
        ),
        default
    );

    // Suggestions do not silently overwrite the global preference.
    let slack = AppIdentity {
        x11_window_class: Some("Slack".into()),
        ..AppIdentity::default()
    };
    assert_eq!(resolve_shape(Some(&slack), &[], &default), default);

    let misleading = AppIdentity {
        macos_bundle_id: Some("com.example.client".into()),
        display_name: Some("Terminal".into()),
        ..AppIdentity::default()
    };
    assert_eq!(resolve_shape(Some(&misleading), &[], &default), default);
}

#[test]
fn stored_values_round_trip_as_camel_case() {
    let shape = ShapePreset::ChatMessage.shape();
    let value = serde_json::to_value(shape).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "preset": "chatMessage",
            "capitals": "relaxed",
            "dropFinalPeriod": true,
            "tidy": true,
            "spokenPunctuation": true,
            "lineBreaks": "spoken",
            "rewriteWithLocalModel": false,
        })
    );
    assert_eq!(serde_json::from_value::<Shape>(value).unwrap(), shape);
    let rule = AppRule {
        shape,
        app: AppMatcher::MacosBundleId("com.apple.mail".into()),
    };
    let value = serde_json::to_value(&rule).unwrap();
    assert_eq!(
        value["app"],
        serde_json::json!({"kind": "macosBundleId", "value": "com.apple.mail"})
    );
    assert_eq!(serde_json::from_value::<AppRule>(value).unwrap(), rule);
}

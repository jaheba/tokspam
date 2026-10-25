use tokspam::{Checker, Limits, Reason, check};

#[test]
fn unrecognized_ascii_code_bytes_are_rejected_at_their_source_range() {
    let limits = Limits::screening();
    let mut checker = Checker::new(limits.clone());
    let invalid = (0u8..=31)
        .filter(|byte| !matches!(byte, b'\t' | b'\n' | 0x0c | b'\r'))
        .chain([0x7f, b'$', b'?', b'`', b'\\']);

    for byte in invalid {
        let character = char::from(byte);
        for source in [
            format!("value = {character} 1\n"),
            format!("value = [{character}]\n"),
            format!("value = f'{{{character}}}'\n"),
            format!("π = 1\n{character}"),
        ] {
            let offset = source.find(character).unwrap();
            let error = checker.check(&source).unwrap_err();
            assert_eq!(error.reason, Reason::LexicalStructure, "{source:?}");
            assert_eq!(error.range, offset..offset + 1, "{source:?}");
            assert_eq!(check(&source, &limits), Err(error.clone()));
            #[cfg(feature = "regions")]
            assert_eq!(checker.check_regions(&source), Err(error));

            // Lexical rejection must not poison the next use of this worker.
            assert!(checker.check("value = 1\n").is_ok());
        }
    }
}

#[test]
fn literal_and_comment_text_and_supported_punctuation_are_preserved() {
    let mut checker = Checker::new(Limits::screening());
    for source in [
        "message = '$?`'\n",
        "message = b'$?`'\n",
        "message = r'$?`'\n",
        "message = '''$?`\ntext'''\n",
        "# $ ? ` \\\nvalue = 1\n",
        "message = f'$?` {value!r}'\n",
        "message = t'$?` {value!s}'\n",
        "message = f'{value:$>10}'\n",
        "value = 1 != 2\n",
        "value = 1 + \\\n2\n",
        "value = 1 + \\\r\n2\n",
        "π = 1\n",
    ] {
        assert!(checker.check(source).is_ok(), "{source:?}");
        #[cfg(feature = "regions")]
        assert!(checker.check_regions(source).is_ok(), "{source:?}");
    }
}

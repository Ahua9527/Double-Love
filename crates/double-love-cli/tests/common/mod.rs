pub fn missing_test_tools(reason: &str) {
    assert!(
        std::env::var("DOUBLELOVE_REQUIRE_TEST_TOOLS").as_deref() != Ok("1"),
        "required test tools unavailable: {reason}"
    );
    eprintln!("skip: {reason}");
}

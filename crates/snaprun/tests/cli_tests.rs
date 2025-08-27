#[test]
fn cli_tests() {
    trycmd::TestCases::new()
        .case("tests/cmd/*.toml")
        .case("test/cmd/*.trycmd")
        .case("README.md");
}

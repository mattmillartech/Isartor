#[tokio::test]
#[ignore = "requires GITHUB_TOKEN with active Copilot access"]
async fn copilot_token_validation_succeeds() {
    let github_token =
        std::env::var("GITHUB_TOKEN").expect("set GITHUB_TOKEN to run this integration test");
    let client = reqwest::Client::new();

    isartor::providers::copilot::CopilotAgent::validate_github_token(&client, &github_token)
        .await
        .expect("GitHub token should validate against the Copilot subscription endpoint");
}

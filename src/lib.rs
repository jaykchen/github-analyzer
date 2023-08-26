pub mod data_analyzers;
pub mod github_data_fetchers;
pub mod octocrab_compat;
pub mod reports;
pub mod utils;
use chrono::{Duration, Utc};
use data_analyzers::*;
use dotenv::dotenv;
use flowsnet_platform_sdk::logger;
use github_data_fetchers::*;
use serde_json::Value;
use slack_flows::send_message_to_channel;
use std::env;
use std::{collections::HashMap, thread::sleep};
use webhook_flows::{request_received, send_response};
#[no_mangle]
#[tokio::main(flavor = "current_thread")]
pub async fn run() {
    dotenv().ok();
    logger::init();

    request_received(handler).await;
}

async fn handler(_headers: Vec<(String, String)>, _qry: HashMap<String, Value>, _body: Vec<u8>) {
    let github_token = env::var("github_token").expect("github_token was not present in env");

    let (owner, repo) = match (
        _qry.get("owner").unwrap_or(&Value::Null).as_str(),
        _qry.get("repo").unwrap_or(&Value::Null).as_str(),
    ) {
        (Some(o), Some(r)) => (o.to_string(), r.to_string()),
        (_, _) => {
            send_response(
                400,
                vec![(String::from("content-type"), String::from("text/plain"))],
                "You must provide an owner and repo name."
                    .as_bytes()
                    .to_vec(),
            );
            return;
        }
    };

    let user_name = _qry
        .get("username")
        .unwrap_or(&Value::Null)
        .as_str()
        .map(|n| n.to_string());

    let n_days = 7u16;
    let mut report = Vec::<String>::new();

    let mut _profile_data = String::new();

    match is_valid_owner_repo_integrated(&github_token, &owner, &repo).await {
        None => {
            send_response(
                400,
                vec![(String::from("content-type"), String::from("text/plain"))],
                "You've entered invalid owner/repo, or the target is private. Please try again."
                    .as_bytes()
                    .to_vec(),
            );
            return;
        }
        Some(gm) => {
            _profile_data = format!("About {}/{}: {}", owner, repo, gm.payload);
        }
    }

    // match &user_name {
    //     Some(user_name) => {
    //         if !is_code_contributor(&github_token, &owner, &repo, user_name).await {}
    //     }
    //     None => send_message_to_channel("ik8", "ch_pro", "no_username".to_string()).await,
    // }

    let mut commits_count = 0;
    let mut issues_count = 0;

    let mut commits_summaries = String::new();
    'commits_block: {
        match get_commits_in_range(&github_token, &owner, &repo, user_name.clone(), n_days).await {
            Some((count, mut commits_vec, weekly_commits_vec)) => {
                let commits_str = commits_vec
                    .iter()
                    .map(|com| com.source_url.to_owned())
                    .collect::<Vec<String>>()
                    .join("\n");

                report.push(format!("found {count} commits:\n{commits_str}"));
                // send_message_to_channel("ik8", "ch_rep", commits_str.to_string()).await;
                let mut is_sparce = false;
                let mut turbo = false;
                match count {
                    0 => break 'commits_block,
                    1..=2 => is_sparce = true,
                    6.. => turbo = true,
                    _ => {}
                };
                commits_count = count;
                match process_commits(&github_token, &mut commits_vec, turbo, is_sparce).await {
                    Some(summary) => {
                        commits_summaries = summary;
                    }
                    None => log::error!("processing commits failed"),
                }

                if is_sparce {
                    let weekly_commits_log = weekly_commits_vec
                        .iter()
                        .map(|com| format!("{}: {}", com.name, com.tag_line))
                        .collect::<Vec<String>>()
                        .join("\n");

                    commits_summaries = format!("Here is the contributor's commits details: {commits_summaries}, here is the log of weekly commits for the entire repository: {weekly_commits_log}");
                }
                send_message_to_channel("ik8", "ch_rep", commits_summaries.clone()).await;
            }
            None => log::error!("failed to get commits"),
        }
    }
    let mut issues_summaries = String::new();

    'issues_block: {
        match get_issues_in_range(&github_token, &owner, &repo, user_name.clone(), n_days).await {
            Some((count, issue_vec)) => {
                let issues_str = issue_vec
                    .iter()
                    .map(|issue| issue.html_url.to_owned())
                    .collect::<Vec<String>>()
                    .join("\n");

                report.push(format!("found {count} issues:\n{issues_str}"));
                // send_message_to_channel("ik8", "ch_iss", issues_str.to_string()).await;

                let mut is_sparce = false;
                let mut turbo = false;

                match count {
                    0 => break 'issues_block,
                    1..=2 => is_sparce = true,
                    4.. => turbo = true,
                    _ => {}
                };
                issues_count = count;
                match process_issues(
                    &github_token,
                    issue_vec,
                    user_name.clone(),
                    turbo,
                    is_sparce,
                )
                .await
                {
                    Some((summary, _, issues_vec)) => {
                        send_message_to_channel("ik8", "ch_iss", summary.clone()).await;
                        issues_summaries = summary;
                    }
                    None => log::error!("processing issues failed"),
                }
            }
            None => log::error!("failed to get issues"),
        }
    }

    let now = Utc::now();
    let a_week_ago = now - Duration::days(n_days as i64 + 30);
    let n_days_ago_str = a_week_ago.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let discussion_query = match &user_name {
        Some(user_name) => {
            format!("repo:{owner}/{repo} involves: {user_name} updated:>{n_days_ago_str}")
        }
        None => format!("repo:{owner}/{repo} updated:>{n_days_ago_str}"),
    };

    let mut discussion_data = String::new();
    match search_discussions_integrated(&github_token, &discussion_query, &user_name).await {
        Some((summary, discussion_vec)) => {
            let count = discussion_vec.len();
            let discussions_str = discussion_vec
                .iter()
                .map(|discussion| discussion.source_url.to_owned())
                .collect::<Vec<String>>()
                .join("\n");

            report.push(format!(
                "{count} discussions were referenced in analysis:\n {discussions_str}"
            ));
            // send_message_to_channel("ik8", "ch_dis", summary.clone()).await;
            discussion_data = summary;
        }
        None => log::error!("failed to get discussions"),
    }

    let is_jumbo = (commits_count + issues_count) > 15;

    if commits_summaries.is_empty() && issues_summaries.is_empty() && discussion_data.is_empty() {
        match &user_name {
            Some(target_person) => {
                report = vec![format!(
                    "No useful data found for {}, you may try alternative means to find out more about {}",
                    target_person, target_person
                )];
            }

            None => {
                report = vec!["No useful data found, nothing to report".to_string()];
            }
        }
    } else {
        match correlate_commits_issues_discussions(
            Some(&_profile_data),
            Some(&commits_summaries),
            Some(&issues_summaries),
            Some(&discussion_data),
            user_name.as_deref(),
            is_jumbo,
        )
        .await
        {
            None => {
                report = vec!["no report generated".to_string()];
            }
            Some(final_summary) => {
                report.push(final_summary);
            }
        }
    }

    let output = report.join("\n");
    send_message_to_channel("ik8", "general", output.clone()).await;

    send_response(
        200,
        vec![(String::from("content-type"), String::from("text/plain"))],
        output.as_bytes().to_vec(),
    );
}

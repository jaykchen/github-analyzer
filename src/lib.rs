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

    let start_msg_str = match &user_name {
        Some(name) => format!(
            "Processing data for owner: {}, repo: {}, and user: {}",
            owner, repo, name
        ),
        None => format!(
            "You didn't input a user's name. Bot will then create a report on the weekly progress of {}/{}.",
            owner, repo
        ),
    };
    send_response(
        200,
        vec![(String::from("content-type"), String::from("text/plain"))],
        start_msg_str.as_bytes().to_vec(),
    );

    let mut message_que = std::collections::VecDeque::<String>::new();

    let n_days = 30u16;
    let mut report = Vec::<String>::new();

    let mut _profile_data = String::new();
    match is_valid_owner_repo(&github_token, &owner, &repo).await {
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

    match &user_name {
        Some(user_name) => {
            if !is_code_contributor(&github_token, &owner, &repo, &user_name.clone()).await {
                send_response(
                    200,
                    vec![(String::from("content-type"), String::from("text/plain"))],
                    format!(
                        "{} hasn't contributed code to {}/{}. Bot will try to find out {}'s other contributions.",
                        user_name, owner, repo, user_name
                    ).as_bytes()
                    .to_vec(),

                );
            }
        }
        None => {
            send_response(
                200,
                vec![(String::from("content-type"), String::from("text/plain"))],
                format!(
                    "You didn't input a user's name. Bot will then create a report on the weekly progress of {}/{}.",
                    owner, repo
                ).as_bytes()
                .to_vec(),

            );
        }
    }

    let addressee_str = match &user_name {
        Some(user_name) => format!("{}'s", user_name),
        None => String::from("key community participants'"),
    };

    let start_msg_str =
        format!("exploring {addressee_str} GitHub contributions to `{owner}/{repo}` project");

    // send_response(
    //     200,
    //     vec![(String::from("content-type"), String::from("text/plain"))],
    //     start_msg_str.as_bytes().to_vec(),
    // );

    let mut commits_summaries = String::new();
/*     'commits_block: {
        match get_commits_in_range(
            &github_token,
            &owner,
            &repo,
            Some(&user_name.clone().unwrap().to_string()),
            n_days,
        )
        .await
        {
            Some((count, mut commits_vec)) => {
                let commits_str = commits_vec
                    .iter()
                    .map(|com| {
                        let sha = com
                            .source_url
                            .rsplitn(2, '/')
                            .nth(0)
                            .unwrap_or("1234567")
                            .chars()
                            .take(7)
                            .collect::<String>();

                        format!("<a href=\"{}\">{}</a>", com.source_url, sha)
                    })
                    .collect::<Vec<String>>()
                    .join("\n");
                let commits_head_str = format!("found {count} commits");
                let commits_msg_str = format!("{}:\n{}", commits_head_str.clone(), commits_str);
                report.push(format!("{}", commits_msg_str.clone()));
                send_response(
                    200,
                    vec![(String::from("content-type"), String::from("text/html"))],
                    commits_msg_str.as_bytes().to_vec(),
                );
                if count == 0 {
                    break 'commits_block;
                }
                match process_commits(&github_token, &mut commits_vec).await {
                    Some(summary) => {
                        commits_summaries = summary;
                    }
                    None => log::error!("processing commits failed"),
                }

                if !commits_vec.is_empty() {
                    for com in commits_vec {
                        sleep(std::time::Duration::from_secs(2));
                        send_message_to_channel("ik8", "ch_rep", com.payload).await;
                    }
                }
            }
            None => log::error!("failed to get commits"),
        }
    } */
    let mut issues_summaries = String::new();

/*     'issues_block: {
        match get_issues_in_range(
            &github_token,
            &owner,
            &repo,
            Some(&user_name.clone().unwrap().to_string()),
            n_days,
        )
        .await
        {
            Some((count, issue_vec)) => {
                let issues_str = issue_vec
                    .iter()
                    .map(|issue| issue.url.rsplitn(2, '/').nth(0).unwrap_or("1234"))
                    .collect::<Vec<&str>>()
                    .join(", ");
                let issues_msg_str = format!("found {} issues: {}", count, issues_str);
                report.push(format!("{issues_msg_str}\n"));
                send_response(
                    200,
                    vec![(String::from("content-type"), String::from("text/plain"))],
                    issues_msg_str.as_bytes().to_vec(),
                );
                if count == 0 {
                    break 'issues_block;
                }

                match process_issues(
                    &github_token,
                    issue_vec,
                    Some(&user_name.clone().unwrap().to_string()),
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
    } */

    let now = Utc::now();
    let a_week_ago = now - Duration::days(n_days as i64);
    let n_days_ago_str = a_week_ago.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let discussion_query = match &user_name {
        Some(user_name) => format!("involves: {user_name} updated:>{n_days_ago_str}"),
        None => format!("updated:>{n_days_ago_str}"),
    };

    let mut discussion_data = String::new();
    match search_discussions_integrated(&github_token, &discussion_query, &user_name).await {
        Some((summary, discussion_vec)) => {
            let count = discussion_vec.len();
            let discussions_str = discussion_vec
                .iter()
                .map(|discussion| {
                    discussion
                        .source_url
                        .rsplitn(2, '/')
                        .nth(0)
                        .unwrap_or("1234")
                })
                .collect::<Vec<&str>>()
                .join(", ");
            let discussions_msg_str = format!("found {} discussions:\n {}", count, discussions_str);
            report.push(format!("{discussions_msg_str}\n"));
            send_message_to_channel("ik8", "ch_dis", summary.clone()).await;
            discussion_data = summary;
        }
        None => log::error!("failed to get discussions"),
    }

    if commits_summaries.is_empty() && issues_summaries.is_empty() && discussion_data.is_empty() {
        match &user_name {
            Some(target_person) => {
                report = vec![format!(
                    "No useful data found for {}, you may try `/search` to find out more about {}",
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

    send_response(
        200,
        vec![(String::from("content-type"), String::from("text/plain"))],
        report.join("\n").as_bytes().to_vec(),
    );
}

pub async fn send_response_wrapped(
    mut que: std::collections::VecDeque<String>,
    report: &mut Vec<String>,
) {
    while let Some(msg) = que.pop_back() {
        report.push(msg.clone());

        sleep(std::time::Duration::from_secs(2));

        send_response(
            200,
            vec![(String::from("content-type"), String::from("text/plain"))],
            msg.as_bytes().to_vec(),
        );
    }
}

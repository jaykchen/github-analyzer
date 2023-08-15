/* use crate::data_analyzers::*;
use crate::github_data_fetchers::*;
use chrono::{Duration, Utc};
use log;
use slack_flows::send_message_to_channel;
pub async fn weekly_report(
    owner: &str,
    repo: &str,
    user_name: Option<&str>,
    key: Option<&str>,
) -> Option<String> {
    let key = match key {
        Some(key) => key,
        None => "contributors",
    };
    let (success_or_fail, contributor_count) = populate_contributors(owner, repo).await;
    if !success_or_fail {
        log::error!("weekly_report, failed to populate contributors");
        return None;
    }

    let has_user_name = user_name.is_some();

    let is_code_contributor = is_code_contributor(owner, repo, user_name.unwrap()).await;

    // if has_user_name && is_code_contributor {
    //     return new_contributor_report(owner, repo, user_name.unwrap()).await;
    // }

    // if has_user_name && !is_code_contributor {
    //     return current_contributor_report(owner, repo, user_name.unwrap()).await;
    // }

    // if !has_user_name {
    //     return current_repo_report(owner, repo).await;
    // }
    None // This is the default return for when user_name is None
}

pub async fn new_contributor_report(github_token: &str, owner: &str, repo: &str, user_name: &str) -> Option<String> {
    let mut home_repo_data = get_readme(owner, repo).await.unwrap_or("".to_string());
    match get_community_profile_data(owner, repo).await {
        Some(community_profile_data) => {
            home_repo_data.push_str(&community_profile_data);
        }
        None => {}
    };
    send_message_to_channel("ik8", "ch_home", home_repo_data.clone()).await;
    let user_profile = get_user_data_by_login(user_name)
        .await
        .unwrap_or("".to_string());
    send_message_to_channel("ik8", "ch_pro", user_profile.clone()).await;

    let now = Utc::now();
    let a_week_ago = now - Duration::days(7);
    let a_week_ago_str = a_week_ago.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // current search result may include issues the user interacted much earlier but updated recently
    // may need to do 2 separate searches: "commenter:juntao updated:>2023-07-30T02:49:06Z"
    let issue_query = format!("involves:{user_name} updated:>{a_week_ago_str}");
    let issues_data = search_issue(&issue_query).await.unwrap_or("".to_string());
    let mut repos_data = String::new();

    for language in vec!["rust", "javascript", "cpp", "go"] {
        let temp = get_user_repos_gql(user_name, language)
            .await
            .unwrap_or("".to_string());
        repos_data.push_str(&temp);
    }
    send_message_to_channel("ik8", "ch_rep", repos_data.clone()).await;

    let discussion_query = format!("involves:{user_name} updated:>{a_week_ago_str}");
    let (_, discussion_vec) = search_discussions(&discussion_query).await.unwrap();

    let (discussion_data, _) = analyze_discussions(discussion_vec, Some(user_name)).await;
    send_message_to_channel("ik8", "ch_dis", discussion_data.clone()).await;

    return correlate_user_and_home_project(
        &home_repo_data,
        &user_profile,
        &issues_data,
        &repos_data,
        &discussion_data,
    )
    .await;
}
pub async fn current_contributor_report(
    owner: &str,
    repo: &str,
    user_name: &str,
) -> Option<String> {
    let now = Utc::now();
    let a_week_ago = now - Duration::days(7);
    let a_week_ago_str = a_week_ago.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // let issue_query = format!("involves:{user_name} updated:>{a_week_ago_str}");
    // let issues_data = search_issue(&issue_query).await.unwrap_or("".to_string());

    Some("".to_string())
}
pub async fn current_repo_report(github_token: &str, owner: &str, repo: &str) -> Option<String> {
    Some("".to_string())
}
 */
use crate::github_data_fetchers::*;
use crate::octocrab_compat::Issue;
use crate::utils::*;
use chrono::{DateTime, Utc};
use log;
use serde::Deserialize;

pub async fn is_valid_owner_repo(github_token: &str, owner: &str, repo: &str) -> Option<GitMemory> {
    #[derive(Deserialize)]
    struct CommunityProfile {
        health_percentage: u16,
        description: Option<String>,
        readme: Option<String>,
        updated_at: Option<DateTime<Utc>>,
    }

    let community_profile_url = format!(
        "https://api.github.com/repos/{}/{}/community/profile",
        owner, repo
    );

    match github_http_fetch(&github_token, &community_profile_url).await {
        Some(res) => match serde_json::from_slice::<CommunityProfile>(&res) {
            Ok(profile) => {
                let description_content = profile
                    .description
                    .as_ref()
                    .unwrap_or(&String::from(""))
                    .to_string();

                let readme_content = match &profile.readme {
                    Some(_) => get_readme(github_token, owner, repo)
                        .await
                        .unwrap_or_default(),
                    None => description_content.clone(),
                };

                Some(GitMemory {
                    memory_type: MemoryType::Meta,
                    name: format!("{}/{}", owner, repo),
                    tag_line: description_content,
                    source_url: community_profile_url,
                    payload: readme_content,
                    date: profile
                        .updated_at
                        .map_or(Utc::now().date_naive(), |dt| dt.date_naive()),
                })
            }
            Err(e) => {
                log::error!("Error parsing Community Profile: {:?}", e);
                None
            }
        },
        None => {
            log::error!(
                "Error fetching Community Profile: {:?}",
                community_profile_url
            );
            None
        }
    }
}
pub async fn process_issues(
    github_token: &str,
    inp_vec: Vec<Issue>,
    target_person: Option<&str>,
) -> Option<(String, usize, Vec<GitMemory>)> {
    let mut issues_summaries = String::new();
    let mut git_memory_vec = vec![];

    for issue in &inp_vec {
        match get_issue_texts(github_token, issue).await {
            Some(text) => match analyze_issue(issue, target_person, &text).await {
                None => {
                    log::error!("Error analyzing issue: {:?}", issue.url.to_string());
                    continue;
                }
                Some((summary, gm)) => {
                    issues_summaries.push_str(&format!("{} {}\n", gm.date, summary));
                    git_memory_vec.push(gm);
                    if git_memory_vec.len() > 16 {
                        break;
                    }
                }
            },
            None => {
                log::error!("Error getting issue texts: {:?}", issue.url.to_string());
                continue;
            }
        }
    }

    let count = git_memory_vec.len();
    if count == 0 {
        log::error!("No issues processed");
        return None;
    }
    Some((issues_summaries, count, git_memory_vec))
}
pub async fn analyze_issue(
    issue: &Issue,
    target_person: Option<&str>,
    all_text: &str,
) -> Option<(String, GitMemory)> {
    let issue_creator_name = issue.user.login.to_string();
    let issue_number = issue.number;
    let issue_title = issue.title.to_string();

    let issue_date = issue.created_at.date_naive();
    let issue_url = issue.url.to_string();
    let target_str = target_person.unwrap_or("key participants");

    let sys_prompt_1 = &format!(
        "Given the information that user '{issue_creator_name}' opened an issue titled '{issue_title}', your task is to analyze the content of the issue posts. Extract key details including the main problem or question raised, the environment in which the issue occurred, any steps taken by the user and commenters to address the problem, relevant discussions, and any identified solutions, consesus reached, or pending tasks."
    );
    let usr_prompt_1 = &format!(
        "Based on the GitHub issue posts: {all_text}, please list the following key details: The main problem or question raised in the issue. The environment or conditions in which the issue occurred (e.g., hardware, OS). Any steps or actions taken by the user or commenters to address the issue. Key discussions or points of view shared by participants in the issue thread. Any solutions identified, consensus reached, or pending tasks if the issue hasn't been resolved. The role and contribution of the user or commenters in the issue."
    );
    let usr_prompt_2 = &format!(
        "Provide a brief summary highlighting the core problem and emphasize the overarching contribution made by '{target_str}' to the resolution of this issue, ensuring your response stays under 128 tokens."
    );

    match chain_of_chat(
        sys_prompt_1,
        usr_prompt_1,
        &format!("issue_{issue_number}"),
        256,
        usr_prompt_2,
        128,
        &format!("Error generatng issue summary #{issue_number}"),
    )
    .await
    {
        Some(issue_summary) => {
            let mut out = format!("{issue_url} ");
            out.push_str(&issue_summary);
            let name = target_person.unwrap_or(&issue_creator_name).to_string();
            let gm = GitMemory {
                memory_type: MemoryType::Issue,
                name: name,
                tag_line: issue_title,
                source_url: issue_url,
                payload: issue_summary,
                date: issue_date,
            };

            Some((out, gm))
        }
        None => {
            log::error!("Error generating issue summary #{issue_number}");
            None
        }
    }
}

pub async fn analyze_commit(
    github_token: &str,
    user_name: &str,
    tag_line: &str,
    url: &str,
) -> Option<String> {
    let commit_patch_str = format!("{url}.patch");
    match github_http_fetch(&github_token, &commit_patch_str).await {
        Some(res) => {
            let text = String::from_utf8_lossy(res.as_slice()).to_string();

            let mut clean_text = String::with_capacity(text.len());
            let mut inside_diff_block = false;
            
            let max_length = 18_000;

            for line in text.lines() {
                if clean_text.len() + line.len() > max_length {
                    let remaining = max_length - clean_text.len();
                    clean_text.push_str(&line.chars().take(remaining).collect::<String>());
                    break;
                }
            
                if line.starts_with("diff --git") {
                    inside_diff_block = true;
                    clean_text.push_str(line);
                    clean_text.push('\n');
                    continue;
                }
            
                if inside_diff_block {
                    if line.chars().any(|ch| ch == '[' || ch == ']' || ch == '{' || ch == '}') {
                        continue;
                    }
                }
            
                clean_text.push_str(line);
                clean_text.push('\n');
            
                if line.is_empty() {
                    inside_diff_block = false;
                }
            }
            let sys_prompt_1 = &format!(
                "You are provided with a commit patch by the user {user_name}. Your task is to parse this data, focusing on the following sections: the Date Line, Subject Line, Diff Files, Diff Changes, Sign-off Line, and the File Changes Summary. Extract key elements of the commit, and the types of files affected, prioritizing code files, scripts, then documentation. Be particularly careful to distinguish between changes made to core code files and modifications made to documentation files, even if they contain technical content. Compile a list of the extracted key elements."
            );

            let usr_prompt_1 = &format!(
                "Based on the provided commit patch: {clean_text}, and description: {tag_line}, extract and present the following key elements: a high-level summary of the changes made, and the types of files affected. Prioritize data on changes to code files first, then scripts, and lastly documentation. Pay attention to the file types and ensure the distinction between documentation changes and core code changes, even when the documentation contains highly technical language. Please compile your findings into a list, with each key element represented as a separate item."
            );

            let usr_prompt_2 = &format!(
                "Using the key elements you extracted from the commit patch, provide a summary of the user's contributions to the project. Include the types of files affected, and the overall changes made. When describing the affected files, make sure to differentiate between changes to core code files, scripts, and documentation files. Present your summary in this format: '(summary of changes). (overall impact of changes).' Please ensure your answer stayed below 128 tokens."
            );

            let sha_serial = match url.rsplitn(2, "/").nth(0) {
                Some(s) => s.chars().take(5).collect::<String>(),
                None => "0000".to_string(),
            };
            chain_of_chat(
                sys_prompt_1,
                usr_prompt_1,
                &format!("commit-{sha_serial}"),
                256,
                usr_prompt_2,
                128,
                &format!("analyze_commit-{sha_serial}"),
            )
            .await
        }
        None => None,
    }
}

pub async fn process_commits(
    github_token: &str,
    inp_vec: Vec<GitMemory>,
) -> Option<(String, usize, Vec<GitMemory>)> {
    let mut commits_summaries = String::new();
    let mut git_memory_vec = vec![];
    let mut inp_vec = inp_vec;
    for commit_obj in inp_vec.drain(..) {
        match analyze_commit(
            github_token,
            &commit_obj.name,
            &commit_obj.tag_line,
            &commit_obj.source_url,
        )
        .await
        {
            Some(summary) => {
                let mut commit_obj = commit_obj; // to make it mutable
                commit_obj.payload = summary;

                if commits_summaries.len() <= 45_000 {
                    commits_summaries
                        .push_str(&format!("{} {}\n", commit_obj.date, commit_obj.payload));
                }

                git_memory_vec.push(commit_obj);
                if git_memory_vec.len() > 20 {
                    break;
                }
            }
            None => {
                log::error!(
                    "Error analyzing commit {:?} for user {}",
                    commit_obj.source_url,
                    commit_obj.name
                );
            }
        }
    }

    let count = git_memory_vec.len();
    if count == 0 {
        log::error!("No commits processed");
        return None;
    }
    Some((commits_summaries, count, git_memory_vec))
}

pub async fn analyze_discussions(
    mut discussions: Vec<GitMemory>,
    target_person: Option<&str>,
) -> (String, Vec<GitMemory>) {
    let target_str = target_person.unwrap_or("key participants");
    let sys_prompt_1 =
        "Given the information on a GitHub discussion, your task is to analyze the content of the discussion posts. Extract key details including the main topic or question raised, any steps taken by the original author and commenters to address the problem, relevant discussions, and any identified solutions, consensus reached, or pending tasks.";

    let mut text_out = "".to_string();
    for gm in discussions.iter_mut() {
        let usr_prompt_1 = &format!(
            "Based on the GitHub discussion post: {}, please list the following key details: The main topic or question raised in the discussion. Any steps or actions taken by the original author or commenters to address the discussion. Key discussions or points of view shared by participants in the discussion thread. Any solutions identified, consensus reached, or pending tasks if the discussion hasn't been resolved. The role and contribution of the user or commenters in the discussion.",
            gm.payload
        );

        let usr_prompt_2 = &format!(
            "Provide a brief summary highlighting the core topic and emphasize the overarching contribution made by '{target_str}' to the resolution of this discussion, ensuring your response stays under 128 tokens."
        );

        let discussion_summary = chain_of_chat(
            sys_prompt_1,
            usr_prompt_1,
            "discussion99",
            256,
            usr_prompt_2,
            128,
            &format!("Error generating discussion summary #{}", gm.source_url),
        )
        .await;

        if let Some(summary) = discussion_summary {
            let out = format!("{} {}", gm.source_url, summary);
            text_out.push_str(&out);
            gm.payload = out;
            if let Some(target) = target_person {
                gm.name = target.to_string();
            }
        } else {
            log::error!("Error generating discussion summary #{}", gm.source_url);
        }
    }

    (text_out, discussions)
}

pub async fn correlate_commits_issues(
    _commits_summary: &str,
    _issues_summary: &str,
) -> Option<String> {
    let (commits_summary, issues_summary) =
        squeeze_fit_commits_issues(_commits_summary, _issues_summary, 0.6);

    let sys_prompt_1 = &format!(
        "Your task is to identify the 1-3 most impactful contributions by a specific user, based on the given commit logs and issue records. Pay close attention to any sequential relationships between issues and commits, and consider how they reflect the user's growth and evolution within the project. Use this data to evaluate the user's overall influence on the project's development. Provide a concise summary in bullet-point format."
    );

    let usr_prompt_1 = &format!(
        "Given the commit logs: {commits_summary} and issue records: {issues_summary}, identify the most significant contributions made by the user. Look for patterns and sequences of events that indicate the user's growth and how they approached problem-solving. Consider major code changes, and initiatives that had substantial impact on the project. Additionally, note any instances where the resolution of an issue led to a specific commit."
    );

    let usr_prompt_2 = &format!(
        "Based on the contributions identified, create a concise bullet-point summary. Highlight the user's key contributions and their influence on the project. Pay attention to their growth over time, and how their responses to issues evolved. Make sure to reference any interconnected events between issues and commits. Avoid replicating phrases from the source data and focus on providing a unique and insightful narrative. Please ensure your answer stayed below 256 tokens."
    );

    chain_of_chat(
        sys_prompt_1,
        usr_prompt_1,
        "correlate-99",
        512,
        usr_prompt_2,
        256,
        "correlate_commits_issues",
    )
    .await
}

pub async fn correlate_commits_issues_discussions(
    _profile_data: Option<&str>,
    _commits_summary: Option<&str>,
    _issues_summary: Option<&str>,
    _discussions_summary: Option<&str>,
    target_person: Option<&str>,
) -> Option<String> {
    let total_space = 16000; // 16k tokens

    let _total_ratio = 11.0; // 1 + 4 + 4 + 2
    let profile_ratio = 1.0;
    let commit_ratio = 4.0;
    let issue_ratio = 4.0;
    let discussion_ratio = 2.0;

    let available_ratios = [
        _profile_data.map(|_| profile_ratio),
        _commits_summary.map(|_| commit_ratio),
        _issues_summary.map(|_| issue_ratio),
        _discussions_summary.map(|_| discussion_ratio),
    ];

    let total_available_ratio: f32 = available_ratios.iter().filter_map(|&x| x).sum();

    let compute_space =
        |ratio: f32| -> usize { ((total_space as f32) * (ratio / total_available_ratio)) as usize };

    let profile_space = _profile_data.map_or(0, |_| compute_space(profile_ratio));
    let commit_space = _commits_summary.map_or(0, |_| compute_space(commit_ratio));
    let issue_space = _issues_summary.map_or(0, |_| compute_space(issue_ratio));
    let discussion_space = _discussions_summary.map_or(0, |_| compute_space(discussion_ratio));

    let trim_to_allocated_space =
        |source: &str, space: usize| -> String { source.chars().take(space * 3).collect() };

    let profile_str = _profile_data.map_or("".to_string(), |x| {
        format!(
            "profile data: {}",
            trim_to_allocated_space(x, profile_space)
        )
    });
    let commits_str = _commits_summary.map_or("".to_string(), |x| {
        format!("commit logs: {}", trim_to_allocated_space(x, commit_space))
    });
    let issues_str = _issues_summary.map_or("".to_string(), |x| {
        format!("issue post: {}", trim_to_allocated_space(x, issue_space))
    });
    let discussions_str = _discussions_summary.map_or("".to_string(), |x| {
        format!(
            "discussion posts: {}",
            trim_to_allocated_space(x, discussion_space)
        )
    });

    let target_str = match target_person {
        Some(person) => format!("{}'s", person),
        None => "key participants'".to_string(),
    };

    let sys_prompt_1 =
        "Analyze the GitHub activity data and profile data over the week to detect both key impactful contributions and connections between commits, issues, and discussions. Highlight specific code changes, resolutions, and improvements. Furthermore, trace evidence of commits addressing specific issues, discussions leading to commits, or issues spurred by discussions. The aim is to map out both the impactful technical advancements and the developmental narrative of the project.";

    let usr_prompt_1 = &format!(
        "From {profile_str}, {commits_str}, {issues_str}, and {discussions_str}, detail {target_str}'s significant technical contributions. Enumerate individual tasks, code enhancements, and bug resolutions, emphasizing impactful contributions. Concurrently, identify connections: commits that appear to resolve specific issues, discussions that may have catalyzed certain commits, or issues influenced by preceding discussions. Extract tangible instances showcasing both impact and interconnections within the week."
    );

    let usr_prompt_2 = &format!(
        "Merge the identified impactful technical contributions and their interconnections into a coherent summary for {target_str} over the week. Describe how these contributions align with the project's technical objectives. Pinpoint recurring technical patterns or trends and shed light on the synergy between individual efforts and their collective progression. Detail both the weight of each contribution and their interconnectedness in shaping the project. Limit to 256 tokens."
    );

    chain_of_chat(
        sys_prompt_1,
        usr_prompt_1,
        "correlate-99",
        512,
        usr_prompt_2,
        256,
        "correlate_commits_issues_discussions",
    )
    .await
}

pub async fn correlate_user_and_home_project(
    home_repo_data: &str,
    user_profile: &str,
    issues_data: &str,
    repos_data: &str,
    discussion_data: &str,
) -> Option<String> {
    let home_repo_data = home_repo_data.chars().take(6000).collect::<String>();
    let user_profile = user_profile.chars().take(4000).collect::<String>();
    let issues_data = issues_data.chars().take(9000).collect::<String>();
    let repos_data = repos_data.chars().take(6000).collect::<String>();
    let discussion_data = discussion_data.chars().take(4000).collect::<String>();

    let sys_prompt_1 = &format!(
        "First, let's analyze and understand the provided Github data in a step-by-step manner. Begin by evaluating the user's activity based on their most active repositories, languages used, issues they're involved in, and discussions they've participated in. Concurrently, grasp the characteristics and requirements of the home project. Your aim is to identify overlaps or connections between the user's skills or activities and the home project's needs."
    );

    let usr_prompt_1 = &format!(
        "Using a structured approach, analyze the given data: User Profile: {} Active Repositories: {} Issues Involved: {} Discussions Participated: {} Home project's characteristics: {} Identify patterns in the user's activity and spot potential synergies with the home project. Pay special attention to the programming languages they use, especially if they align with the home project's requirements. Derive insights from their interactions and the data provided.",
        user_profile,
        repos_data,
        issues_data,
        discussion_data,
        home_repo_data
    );

    let usr_prompt_2 = &format!(
        "Now, using the insights from your step-by-step analysis, craft a concise bullet-point summary that underscores: - The user's main areas of expertise and interest. - The relevance of their preferred languages or technologies to the home project. - Their potential contributions to the home project, based on their skills and interactions. Ensure the summary is clear, insightful, and remains under 256 tokens. Emphasize any evident alignments between the user's skills and the project's needs."
    );
    chain_of_chat(
        sys_prompt_1,
        usr_prompt_1,
        "correlate-user-home",
        512,
        usr_prompt_2,
        256,
        "correlate-user-home-summary",
    )
    .await
}

use crate::octocrab_compat::{Comment, Issue, Repository, User};
use crate::utils::*;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use derivative::Derivative;
use http_req::response::Response;
use openai_flows::{
    self,
    chat::{self, ChatOptions},
    OpenAIFlows,
};
use serde::{Deserialize, Serialize};
use serde_json;
use store_flows::{get, set};

#[derive(Derivative, Serialize, Deserialize, Debug)]
pub struct GitMemory {
    pub memory_type: MemoryType,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub name: String,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub tag_line: String,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub source_url: String,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub payload: String,
    pub date: NaiveDate,
}
#[derive(Serialize, Deserialize, Debug)]
pub enum MemoryType {
    Commit,
    Issue,
    Discussion,
    Meta,
}
pub async fn get_user_profile(github_token: &str, user: &str) -> Option<User> {
    let user_profile_url = format!("https://api.github.com/users/{user}");

    match github_http_fetch(&github_token, &user_profile_url).await {
        Some(res) => serde_json::from_slice::<User>(res.as_slice()).ok(),

        None => {
            log::error!("Github user not found.");
            None
        }
    }
}
pub async fn get_user_data_by_login(github_token: &str, login: &str) -> Option<String> {
    #[derive(Debug, Deserialize)]
    struct User {
        name: Option<String>,
        login: Option<String>,
        url: Option<String>,
        #[serde(rename = "twitterUsername")]
        twitter_username: Option<String>,
        bio: Option<String>,
        company: Option<String>,
        location: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: Option<DateTime<Utc>>,
        email: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct RepositoryOwner {
        #[serde(rename = "repositoryOwner")]
        repository_owner: Option<User>,
    }

    #[derive(Debug, Deserialize)]
    struct UserRoot {
        data: Option<RepositoryOwner>,
    }

    let base_url = "https://api.github.com/graphql";
    let mut out = String::from("USER_profile: \n");

    let query = format!(
        r#"
        query {{
            repositoryOwner(login: "{login}") {{
                ... on User {{
                    name
                    login
                    url
                    twitterUsername
                    bio
                    company
                    location
                    createdAt
                    email
                }}
            }}
        }}
        "#
    );

    match github_http_post(&github_token, base_url, &&query).await {
        None => {
            log::info!("Failed to send the request to get UserRoot: {}", base_url);
            return None;
        }
        Some(res) => match serde_json::from_slice::<UserRoot>(res.as_slice()) {
            Err(e) => {
                log::error!("Failed to parse the response for UserRoot: {}", e);
                return None;
            }
            Ok(results) => {
                if let Some(repository_owner) = &results.data {
                    if let Some(user) = &repository_owner.repository_owner {
                        let login_str = match &user.login {
                            Some(login) => format!("Login: {},", login),
                            None => {
                                return None;
                            }
                        };

                        let name_str = match &user.name {
                            Some(name) => format!("Name: {},", name),
                            None => String::new(),
                        };

                        let url_str = match &user.url {
                            Some(url) => format!("Url: {},", url),
                            None => String::new(),
                        };

                        let twitter_str = match &user.twitter_username {
                            Some(twitter) => format!("Twitter: {},", twitter),
                            None => String::new(),
                        };

                        let bio_str = match &user.bio {
                            Some(bio) if bio.is_empty() => String::new(),
                            Some(bio) => format!("Bio: {},", bio),
                            None => String::new(),
                        };

                        let company_str = match &user.company {
                            Some(company) => format!("Company: {},", company),
                            None => String::new(),
                        };

                        let location_str = match &user.location {
                            Some(location) => format!("Location: {},", location),
                            None => String::new(),
                        };

                        let date_str = match &user.created_at {
                            Some(date) => {
                                format!("Created At: {},", date.date_naive().to_string())
                            }
                            None => String::new(),
                        };

                        let email_str = match &user.email {
                            Some(email) => format!("Email: {}", email),
                            None => String::new(),
                        };

                        out.push_str(
                                &format!(
                                    "{name_str} {login_str} {url_str} {twitter_str} {bio_str} {company_str} {location_str} {date_str} {email_str}\n"
                                )
                            );
                    }
                }
            }
        },
    }

    Some(out)
}
pub async fn get_community_profile_data(
    github_token: &str,
    owner: &str,
    repo: &str,
) -> Option<String> {
    #[derive(Deserialize, Debug)]
    struct CommunityProfile {
        description: String,
        // documentation: Option<String>,
    }

    let community_profile_url =
        format!("https://api.github.com/repos/{owner}/{repo}/community/profile");

    match github_http_fetch(&github_token, &community_profile_url).await {
        Some(res) => match serde_json::from_slice::<CommunityProfile>(&res) {
            Ok(profile) => {
                return Some(format!("Description: {}", profile.description));
            }
            Err(e) => log::error!("Error parsing Community Profile: {:?}", e),
        },
        None => log::error!("Community profile not found for {}/{}.", owner, repo),
    }
    None
}
pub async fn is_code_contributor(
    github_token: &str,
    owner: &str,
    repo: &str,
    user_name: &str,
) -> bool {
    use std::hash::Hasher;
    use twox_hash::XxHash;
    let repo_string = format!("{owner}/{repo}");
    let mut hasher = XxHash::with_seed(0);
    hasher.write(repo_string.as_bytes());
    let hash = hasher.finish();
    let key = &format!("{:x}", hash);
    match get(key)
        .and_then(|val| serde_json::from_value::<std::collections::HashSet<String>>(val).ok())
    {
        Some(set) => set.contains(user_name),
        None => match get_contributors(github_token, owner, repo).await {
            Some(contributors) => {
                set(
                    key,
                    serde_json::to_value(contributors.clone()).unwrap_or_default(),
                    None,
                );
                return contributors.contains(&user_name.to_owned());
            }
            None => {
                log::error!("Github contributors not found.");
                return false;
            }
        },
    }
}

pub async fn get_contributors(github_token: &str, owner: &str, repo: &str) -> Option<Vec<String>> {
    #[derive(Debug, Deserialize)]
    struct GithubUser {
        login: String,
    }

    let url = format!(
        "https://api.github.com/repos/{}/{}/contributors",
        owner, repo
    );
    let mut contributors = Vec::new();

    let mut current_url = url.to_owned();
    loop {
        let response_result: Result<(Response, Vec<u8>), Box<dyn std::error::Error>> =
            github_fetch_with_header(&github_token, &current_url);
        match response_result {
            Err(e) => {
                log::error!(
                    "Error getting response for request to get contributors: {:?}",
                    e
                );
                return None;
            }
            Ok((res, body)) => {
                let status = res.status_code(); // Check the status code
                if !status.is_success() {
                    log::error!(
                        "Request to get contributors, unexpected status code: {:?}",
                        status
                    );
                    return None;
                }

                let new_contributors: Vec<GithubUser> =
                    match serde_json::from_slice(body.as_slice()) {
                        Ok(contributors) => contributors,
                        Err(err) => {
                            log::error!("Error parsing contributors: {:?}", err);
                            return None;
                        }
                    };

                contributors.extend(new_contributors.into_iter().map(|user| user.login));

                // Handle pagination
                let res_headers = res.headers();
                let link_header = res_headers.get("Link");
                match link_header {
                    Some(header) => {
                        let next_link_temp: Option<String> = header
                            .as_str()
                            .split(',')
                            .filter_map(|link| {
                                if link.contains("rel=\"next\"") {
                                    link.split(';').next().map(|url| {
                                        url.trim_matches(&[' ', '<', '>'] as &[char]).to_string()
                                    })
                                } else {
                                    None
                                }
                            })
                            .next();

                        let next_link = next_link_temp.as_deref();

                        if let Some(link) = next_link {
                            current_url = link.to_string();
                        } else {
                            break;
                        }
                    }
                    None => {
                        break;
                    }
                }
            }
        }
    }

    Some(contributors)
}

pub async fn get_readme(github_token: &str, owner: &str, repo: &str) -> Option<String> {
    #[derive(Deserialize, Debug)]
    struct GithubReadme {
        content: Option<String>,
    }

    let readme_url = format!("https://api.github.com/repos/{owner}/{repo}/readme");

    match github_http_fetch(&github_token, &readme_url).await {
        Some(res) => match serde_json::from_slice::<GithubReadme>(&res) {
            Ok(readme) => {
                if let Some(c) = readme.content {
                    let cleaned_content = c.replace("\n", "");
                    match base64::decode(&cleaned_content) {
                        Ok(decoded_content) => match String::from_utf8(decoded_content) {
                            Ok(out) => {
                                return Some(format!("Readme: {}", out));
                            }
                            Err(e) => {
                                log::error!("Failed to convert cleaned readme to String: {:?}", e);
                                return None;
                            }
                        },
                        Err(e) => {
                            log::error!("Error decoding base64 content: {:?}", e);
                            None
                        }
                    }
                } else {
                    log::error!("Content field in readme is null.");
                    None
                }
            }
            Err(e) => {
                log::error!("Error parsing Readme: {:?}", e);
                None
            }
        },
        None => {
            log::error!("Github readme not found.");
            None
        }
    }
}

pub async fn get_issues_in_range(
    github_token: &str,
    owner: &str,
    repo: &str,
    user_name: Option<String>,
    range: u16,
) -> Option<(usize, Vec<Issue>)> {
    #[derive(Debug, Deserialize)]
    struct Page<T> {
        pub items: Vec<T>,
        pub total_count: Option<u64>,
    }

    let now = Utc::now();
    let n_days_ago = (now - Duration::days(range as i64))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let user_str = user_name
        .map(|u| format!("involves:{}", u))
        .unwrap_or_default();

    let query = format!("repo:{owner}/{repo} is:issue {user_str} updated:>{n_days_ago}");
    let encoded_query = urlencoding::encode(&query);

    let mut issue_vec = vec![];
    let mut total_pages = None;
    let mut current_page = 1;

    loop {
        let url_str = format!(
            "https://api.github.com/search/issues?q={}&sort=updated&order=desc&page={}",
            encoded_query, current_page
        );

        match github_http_fetch(&github_token, &url_str).await {
            Some(res) => match serde_json::from_slice::<Page<Issue>>(res.as_slice()) {
                Err(e) => {
                    log::error!("error: {:?}", e);
                    break;
                }
                Ok(issue_page) => {
                    if total_pages.is_none() {
                        if let Some(total) = issue_page.total_count {
                            total_pages = Some(((total as f64) / 30.0).ceil() as usize);
                        }
                    }

                    for issue in issue_page.items {
                        issue_vec.push(issue.clone());
                    }

                    current_page += 1;
                    if current_page > total_pages.unwrap_or(usize::MAX) {
                        break;
                    }
                }
            },
            None => {
                break;
            }
        }
    }
    let count = issue_vec.len();
    Some((count, issue_vec))
}

pub async fn get_issue_texts(github_token: &str, issue: &Issue) -> Option<String> {
    let issue_creator_name = &issue.user.login;
    let issue_title = &issue.title;
    let issue_body = match &issue.body {
        Some(body) => squeeze_fit_remove_quoted(body, "```", 500, 0.6),
        None => "".to_string(),
    };
    let issue_url = &issue.url.to_string();

    let labels = issue
        .labels
        .iter()
        .map(|lab| lab.name.clone())
        .collect::<Vec<String>>()
        .join(", ");

    let mut all_text_from_issue = format!(
        "User '{}', opened an issue titled '{}', labeled '{}', with the following post: '{}'.",
        issue_creator_name, issue_title, labels, issue_body
    );

    let mut current_page = 1;
    loop {
        let url_str = format!("{}/comments?&page={}", issue_url, current_page);

        match github_http_fetch(&github_token, &url_str).await {
            Some(res) => match serde_json::from_slice::<Vec<Comment>>(res.as_slice()) {
                Err(_e) => {
                    log::error!(
                        "Error parsing Vec<Comment> at page {}: {:?}",
                        current_page,
                        _e
                    );
                    break;
                }
                Ok(comments_obj) => {
                    if comments_obj.is_empty() {
                        break;
                    }
                    for comment in &comments_obj {
                        let comment_body = match &comment.body {
                            Some(body) => squeeze_fit_remove_quoted(body, "```", 500, 0.6),
                            None => "".to_string(),
                        };
                        let commenter = &comment.user.login;
                        let commenter_input = format!("{} commented: {}", commenter, comment_body);
                        if all_text_from_issue.len() > 45_000 {
                            break;
                        }
                        all_text_from_issue.push_str(&commenter_input);
                    }
                }
            },
            None => {
                break;
            }
        }

        current_page += 1;
    }

    Some(all_text_from_issue)
}

pub async fn get_commits_in_range(
    github_token: &str,
    owner: &str,
    repo: &str,
    user_name: Option<String>,
    range: u16,
) -> Option<(usize, Vec<GitMemory>)> {
    #[derive(Debug, Deserialize, Serialize)]
    struct User {
        login: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct GithubCommit {
        sha: String,
        html_url: String,
        author: Option<User>,    // made nullable
        committer: Option<User>, // made nullable
        commit: CommitDetails,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CommitDetails {
        author: CommitUserDetails,
        message: String,
        // committer: CommitUserDetails,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CommitUserDetails {
        date: Option<DateTime<Utc>>,
    }

    let author_str = match user_name {
        Some(user_name) => format!("author={}", user_name),
        None => "".to_string(),
    };

    let base_commit_url =
        format!("https://api.github.com/repos/{owner}/{repo}/commits?{author_str}");

    let mut git_memory_vec = vec![];
    let now = Utc::now();
    let n_days_ago = (now - Duration::days(range as i64)).date_naive();
    let mut current_page = 1;
    loop {
        let commits_query_url = format!("{base_commit_url}&page={}", current_page);
        match github_http_fetch(&github_token, &commits_query_url).await {
            None => {
                log::error!("Error fetching commits");
                break;
            }
            Some(res) => match serde_json::from_slice::<Vec<GithubCommit>>(res.as_slice()) {
                Err(e) => {
                    log::error!("Error parsing commits: {:?}", e);
                    break;
                }
                Ok(commits) => {
                    if commits.is_empty() {
                        break; // If the page is empty, exit the loop
                    }

                    for commit in commits {
                        if let Some(commit_date) = &commit.commit.author.date {
                            if commit_date.date_naive() > n_days_ago {
                                if let Some(author) = &commit.author {
                                    git_memory_vec.push(GitMemory {
                                        memory_type: MemoryType::Commit,
                                        name: author.login.clone(), // clone as author.login is String type
                                        tag_line: commit.commit.message.clone(),
                                        source_url: commit.html_url.clone(),
                                        payload: String::from(""),
                                        date: commit_date.date_naive(),
                                    });
                                }
                            }
                        }
                    }

                    current_page += 1;
                }
            },
        }
    }

    let count = git_memory_vec.len();
    Some((count, git_memory_vec))
}

pub async fn get_user_repos_in_language(
    github_token: &str,
    user: &str,
    language: &str,
) -> Option<Vec<Repository>> {
    #[derive(Debug, Deserialize)]
    struct Page<T> {
        pub items: Vec<T>,
        pub total_count: Option<u64>,
    }

    let query = format!("user:{} language:{} sort:stars", user, language);
    let encoded_query = urlencoding::encode(&query);

    let mut out: Vec<Repository> = vec![];
    let mut total_pages = None;
    let mut current_page = 1;

    loop {
        let url_str = format!(
            "https://api.github.com/search/repositories?q={}&page={}",
            encoded_query, current_page
        );

        match github_http_fetch(&github_token, &url_str).await {
            Some(res) => match serde_json::from_slice::<Page<Repository>>(res.as_slice()) {
                Err(_e) => {
                    log::error!("Error parsing Page<Repository>: {:?}", _e);
                    break;
                }
                Ok(repo_page) => {
                    if total_pages.is_none() {
                        if let Some(count) = repo_page.total_count {
                            total_pages = Some(((count as f64) / 30.0).ceil() as usize);
                        }
                    }

                    for repo in repo_page.items {
                        out.push(repo);
                    }

                    current_page += 1;
                    if current_page > total_pages.unwrap_or(usize::MAX) {
                        break;
                    }
                }
            },
            None => {
                break;
            }
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub async fn get_user_repos_gql(
    github_token: &str,
    user_name: &str,
    language: &str,
) -> Option<String> {
    #[derive(Debug, Deserialize)]
    struct Root {
        data: Data,
    }

    #[derive(Debug, Deserialize)]
    struct Data {
        search: Search,
    }

    #[derive(Debug, Deserialize)]
    struct Search {
        nodes: Vec<Node>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Node {
        pub name: String,
        #[serde(rename = "defaultBranchRef")]
        default_branch_ref: BranchRef,
        stargazers: Stargazers,
        pub description: Option<String>,
    }
    #[derive(Debug, Deserialize)]
    struct BranchRef {
        target: Target,
    }

    #[derive(Debug, Deserialize)]
    struct Target {
        history: History,
    }

    #[derive(Debug, Deserialize)]
    struct History {
        #[serde(rename = "totalCount")]
        total_count: i32,
    }

    #[derive(Debug, Deserialize)]
    struct Stargazers {
        #[serde(rename = "totalCount")]
        total_count: i32,
    }

    let query = format!(
        r#"
    query {{
        search(query: "user:{} language:{}", type: REPOSITORY, first: 100) {{
            nodes {{
                ... on Repository {{
                    name
                    defaultBranchRef {{
                        target {{
                            ... on Commit {{
                                history(first: 0) {{
                                    totalCount
                                }}
                            }}
                        }}
                    }}
                    description
                    stargazers {{
                        totalCount
                    }}
                }}
            }}
        }}
    }}
    "#,
        user_name, language
    );

    let base_url = "https://api.github.com/graphql";
    let mut out = format!("Repos in {language}:\n");
    match github_http_post(&github_token, base_url, &query).await {
        None => log::error!("Failed to send the request to {}", base_url.to_string()),
        Some(response) => match serde_json::from_slice::<Root>(response.as_slice()) {
            Err(e) => log::error!("Failed to parse the response: {}", e),
            Ok(repos) => {
                let mut repos_sorted: Vec<&Node> = repos.data.search.nodes.iter().collect();
                repos_sorted
                    .sort_by(|a, b| b.stargazers.total_count.cmp(&a.stargazers.total_count));

                for repo in repos_sorted {
                    let name_str = format!("Repo: {}", repo.name);

                    let description_str = match &repo.description {
                        Some(description) => format!("Description: {},", description),
                        None => String::new(),
                    };

                    let stars_str = match repo.stargazers.total_count {
                        0 => String::new(),
                        count => format!("Stars: {count}"),
                    };

                    let commits_str = format!(
                        "Commits: {}",
                        repo.default_branch_ref.target.history.total_count
                    );

                    let temp = format!("{name_str} {description_str} {stars_str} {commits_str}\n");

                    out.push_str(&temp);
                }

                log::info!("Found {} repositories", repos.data.search.nodes.len());
            }
        },
    }
    Some(out)
}

pub async fn search_issue(github_token: &str, search_query: &str) -> Option<String> {
    #[derive(Debug, Deserialize, Clone)]
    pub struct User {
        login: Option<String>,
    }

    #[derive(Debug, Deserialize, Clone)]
    struct AssigneeNode {
        node: Option<User>,
    }

    #[derive(Debug, Deserialize, Clone)]
    struct AssigneeEdge {
        edges: Option<Vec<Option<AssigneeNode>>>,
    }

    #[derive(Debug, Deserialize, Clone)]
    struct Issue {
        url: Option<String>,
        number: Option<u64>,
        state: Option<String>,
        title: Option<String>,
        body: Option<String>,
        author: Option<User>,
        assignees: Option<AssigneeEdge>,
        #[serde(rename = "authorAssociation")]
        author_association: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: Option<DateTime<Utc>>,
        #[serde(rename = "updatedAt")]
        updated_at: Option<DateTime<Utc>>,
    }

    #[derive(Debug, Deserialize)]
    struct IssueNode {
        node: Option<Issue>,
    }

    #[derive(Debug, Deserialize, Clone)]
    struct PageInfo {
        #[serde(rename = "endCursor")]
        end_cursor: Option<String>,
        #[serde(rename = "hasNextPage")]
        has_next_page: Option<bool>,
    }

    #[derive(Debug, Deserialize)]
    struct SearchResult {
        edges: Option<Vec<Option<IssueNode>>>,
        #[serde(rename = "pageInfo")]
        page_info: Option<PageInfo>,
    }

    #[derive(Debug, Deserialize)]
    struct IssueSearch {
        search: Option<SearchResult>,
    }

    #[derive(Debug, Deserialize)]
    struct IssueRoot {
        data: Option<IssueSearch>,
    }

    let base_url = "https://api.github.com/graphql";
    let mut out = String::from("ISSUES \n");

    let mut cursor = None;

    loop {
        let query = format!(
            r#"
            query {{
                search(query: "{search_query}", type: ISSUE, first: 100{after}) {{
                    edges {{
                        node {{
                            ... on Issue {{
                                url
                                number
                                state
                                title
                                body
                                author {{
                                    login
                                }}
                                assignees(first: 100) {{
                                    edges {{
                                        node {{
                                            login
                                        }}
                                    }}
                                }}
                                authorAssociation
                                createdAt
                                updatedAt
                            }}
                        }}
                    }}
                    pageInfo {{
                        endCursor
                        hasNextPage
                      }}
                }}
            }}
            "#,
            search_query = search_query,
            after = cursor
                .as_ref()
                .map_or(String::new(), |c| format!(r#", after: "{}""#, c))
        );

        match github_http_post(&github_token, base_url, &query).await {
            None => {
                log::error!("Failed to send the request: {}", base_url);
                break;
            }
            Some(response) => match serde_json::from_slice::<IssueRoot>(response.as_slice()) {
                Err(e) => {
                    log::error!("Failed to parse the response: {}", e);
                    break;
                }
                Ok(results) => {
                    if let Some(search) = &results.data.as_ref().and_then(|d| d.search.as_ref()) {
                        if let Some(edges) = &search.edges {
                            for edge in edges.iter().filter_map(|e| e.as_ref()) {
                                if let Some(issue) = &edge.node {
                                    let date = match issue.created_at {
                                        Some(date) => date.date_naive().to_string(),
                                        None => {
                                            continue;
                                        }
                                    };
                                    let title_str = match &issue.title {
                                        Some(title) => format!("Title: {},", title),
                                        None => String::new(),
                                    };
                                    let url_str = match &issue.url {
                                        Some(u) => format!("Url: {}", u),
                                        None => String::new(),
                                    };

                                    let author_str =
                                        match issue.clone().author.and_then(|a| a.login) {
                                            Some(auth) => format!("Author: {},", auth),
                                            None => String::new(),
                                        };

                                    let assignees_str = {
                                        let assignee_names = issue
                                            .assignees
                                            .as_ref()
                                            .and_then(|e| e.edges.as_ref())
                                            .map_or(Vec::new(), |assignee_edges| {
                                                assignee_edges
                                                    .iter()
                                                    .filter_map(|edge| {
                                                        edge.as_ref().and_then(|actual_edge| {
                                                            actual_edge.node.as_ref().and_then(
                                                                |user| {
                                                                    user.login.as_ref().map(
                                                                        |login_str| {
                                                                            login_str.as_str()
                                                                        },
                                                                    )
                                                                },
                                                            )
                                                        })
                                                    })
                                                    .collect::<Vec<&str>>()
                                            });

                                        if !assignee_names.is_empty() {
                                            format!("Assignees: {},", assignee_names.join(", "))
                                        } else {
                                            String::new()
                                        }
                                    };

                                    let state_str = match &issue.state {
                                        Some(s) => format!("State: {},", s),
                                        None => String::new(),
                                    };

                                    let body_str = match &issue.body {
                                        Some(body_text) if body_text.len() > 180 => {
                                            let truncated_body = body_text
                                                .chars()
                                                .take(100)
                                                .chain(
                                                    body_text
                                                        .chars()
                                                        .skip(body_text.chars().count() - 80),
                                                )
                                                .collect::<String>();

                                            format!("Body: {}", truncated_body)
                                        }
                                        Some(body_text) => format!("Body: {},", body_text),
                                        None => String::new(),
                                    };

                                    let assoc_str = match &issue.author_association {
                                        Some(association) => {
                                            format!("Author Association: {}", association)
                                        }
                                        None => String::new(),
                                    };

                                    let temp = format!(
                                            "{title_str} {url_str} Created At: {date} {author_str} {assignees_str}  {state_str} {body_str} {assoc_str}"
                                        );

                                    out.push_str(&temp);
                                    out.push_str("\n");
                                } else {
                                    continue;
                                }
                            }
                        }

                        if let Some(page_info) = &search.page_info {
                            if let Some(has_next_page) = page_info.has_next_page {
                                if has_next_page {
                                    match &page_info.end_cursor {
                                        Some(end_cursor) => {
                                            cursor = Some(end_cursor.clone());
                                            log::info!(
                                                    "Fetched a page, moving to next page with cursor: {}",
                                                    end_cursor
                                                );
                                            continue;
                                        }
                                        None => {
                                            log::error!(
                                                    "Warning: hasNextPage is true, but endCursor is None. This might result in missing data."
                                                );
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    break;
                }
            },
        }
    }

    Some(out)
}

pub async fn search_repository(github_token: &str, search_query: &str) -> Option<String> {
    #[derive(Debug, Deserialize)]
    struct Payload {
        data: Option<Data>,
    }

    #[derive(Debug, Deserialize)]
    struct Data {
        search: Option<Search>,
    }

    #[derive(Debug, Deserialize)]
    struct Search {
        edges: Option<Vec<Option<Edge>>>,
        #[serde(rename = "pageInfo")]
        page_info: Option<PageInfo>,
    }

    #[derive(Debug, Deserialize)]
    struct Edge {
        node: Option<Node>,
    }

    #[derive(Debug, Deserialize)]
    struct Node {
        name: Option<String>,
        description: Option<String>,
        url: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: Option<DateTime<Utc>>,
        stargazers: Option<Stargazers>,
        #[serde(rename = "forkCount")]
        fork_count: Option<u32>,
    }

    #[derive(Debug, Deserialize)]
    struct Stargazers {
        #[serde(rename = "totalCount")]
        total_count: Option<u32>,
    }

    #[derive(Debug, Deserialize)]
    struct PageInfo {
        #[serde(rename = "endCursor")]
        end_cursor: Option<String>,
        #[serde(rename = "hasNextPage")]
        has_next_page: Option<bool>,
    }

    let base_url = "https://api.github.com/graphql";
    let mut out = String::from("REPOSITORY \n");

    let mut cursor: Option<String> = None;

    loop {
        let query = format!(
            r#"
                query {{
                    search(query: "{search_query}", type: REPOSITORY, first: 100{after}) {{
                        edges {{
                            node {{
                                ... on Repository {{
                                    name
                                    description
                                    url
                                    createdAt
                                    stargazers {{
                                      totalCount
                                    }}
                                    forkCount
                                }}
                            }}
                        }}
                        pageInfo {{
                            endCursor
                            hasNextPage
                        }}
                    }}
                }}
            "#,
            search_query = search_query,
            after = cursor
                .as_ref()
                .map_or(String::new(), |c| format!(r#", after: "{}""#, c))
        );

        match github_http_post(&github_token, base_url, &query).await {
            None => {
                log::error!(
                    "Failed to send the request to get RepositoryRoot: {}",
                    base_url
                );
                return None;
            }
            Some(response) => match serde_json::from_slice::<Payload>(response.as_slice()) {
                Err(e) => {
                    log::error!("Failed to parse the response for RepositoryRoot: {}", e);
                    return None;
                }
                Ok(payload) => {
                    if let Some(data) = &payload.data {
                        if let Some(search) = &data.search {
                            if let Some(edges) = &search.edges {
                                for edge_option in edges {
                                    if let Some(edge) = edge_option {
                                        if let Some(repo) = &edge.node {
                                            let date_str = match &repo.created_at {
                                                Some(date) => date.date_naive().to_string(),
                                                None => {
                                                    continue;
                                                }
                                            };

                                            let name_str = match &repo.name {
                                                Some(name) => format!("Name: {name},"),
                                                None => String::new(),
                                            };

                                            let desc_str = match &repo.description {
                                                Some(desc) if desc.len() > 300 => {
                                                    let truncated_desc = desc
                                                        .chars()
                                                        .take(180)
                                                        .chain(
                                                            desc.chars()
                                                                .skip(desc.chars().count() - 120),
                                                        )
                                                        .collect::<String>();

                                                    format!("Description: {truncated_desc}")
                                                }
                                                Some(desc) => format!("Description: {desc},"),
                                                None => String::new(),
                                            };

                                            let url_str = match &repo.url {
                                                Some(url) => format!("Url: {url}"),
                                                None => String::new(),
                                            };

                                            let stars_str = match &repo.stargazers {
                                                Some(sg) => format!(
                                                    "Stars: {},",
                                                    sg.total_count.unwrap_or(0)
                                                ),
                                                None => String::new(),
                                            };

                                            let forks_str = match &repo.fork_count {
                                                Some(fork_count) => format!("Forks: {fork_count}"),
                                                None => String::new(),
                                            };

                                            out.push_str(
                                                    &format!(
                                                        "{name_str} {desc_str} {url_str} Created At: {date_str} {stars_str} {forks_str}\n"
                                                    )
                                                );
                                        }
                                    }
                                }
                            }
                            if let Some(page_info) = &search.page_info {
                                if page_info.has_next_page.unwrap_or(false) {
                                    cursor = page_info.end_cursor.clone();
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
            },
        };
    }

    Some(out)
}

pub async fn search_discussions_integrated(
    github_token: &str,
    search_query: &str,
    target_person: &Option<String>,
) -> Option<(String, Vec<GitMemory>)> {
    #[derive(Debug, Deserialize)]
    struct DiscussionRoot {
        data: Option<Data>,
    }

    #[derive(Debug, Deserialize)]
    struct Data {
        search: Option<Search>,
    }

    #[derive(Debug, Deserialize)]
    struct Search {
        edges: Option<Vec<Option<Edge>>>,
    }

    #[derive(Debug, Deserialize)]
    struct Edge {
        node: Option<Discussion>,
    }

    #[derive(Debug, Deserialize)]
    struct Discussion {
        title: Option<String>,
        url: Option<String>,
        html_url: Option<String>,
        author: Option<Author>,
        body: Option<String>,
        comments: Option<Comments>,
        #[serde(rename = "createdAt")]
        created_at: DateTime<Utc>,
        #[serde(rename = "upvoteCount")]
        upvote_count: Option<u32>,
    }

    #[derive(Debug, Deserialize)]
    struct Comments {
        edges: Option<Vec<Option<CommentEdge>>>,
    }

    #[derive(Debug, Deserialize)]
    struct CommentEdge {
        node: Option<CommentNode>,
    }

    #[derive(Debug, Deserialize)]
    struct CommentNode {
        author: Option<Author>,
        body: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct Author {
        login: Option<String>,
    }

    let openai = OpenAIFlows::new();

    let base_url = "https://api.github.com/graphql";

    let query = format!(
        r#"
        query {{
            search(query: "{search_query}", type: DISCUSSION, first: 100) {{
                edges {{
                    node {{
                        ... on Discussion {{
                            title
                            url
                            html_url
                            body
                            author {{
                                login
                            }}
                            createdAt
                            upvoteCount
                            comments (first: 100) {{
                                edges {{
                                    node {{
                                        author {{
                                            login
                                        }}
                                        body
                                    }}
                                }}
                            }}
                        }}
                    }}
                }}
            }}
        }}
        "#,
        search_query = search_query
    );
    let mut git_mem_vec = Vec::with_capacity(100);
    let mut text_out = String::from("DISCUSSIONS \n");

    match github_http_post(&github_token, base_url, &query).await {
        None => {
            log::error!(
                "Failed to send the request to get DiscussionRoot: {}",
                base_url
            );
            return None;
        }
        Some(response) => match serde_json::from_slice::<DiscussionRoot>(&response) {
            Err(e) => {
                log::error!("Failed to parse the response for DiscussionRoot: {}", e);
                return None;
            }
            Ok(results) => {
                let empty_str = "".to_string();

                if let Some(search) = results.data?.search {
                    for edge_option in search.edges?.iter().filter_map(|e| e.as_ref()) {
                        if let Some(discussion) = &edge_option.node {
                            let date = discussion.created_at.date_naive();
                            let title = discussion.title.as_ref().unwrap_or(&empty_str).to_string();
                            let url = discussion.url.as_ref().unwrap_or(&empty_str).to_string();
                            let source_url = discussion.html_url.as_ref().unwrap_or(&empty_str).to_string();
                            let author_login = discussion
                                .author
                                .as_ref()
                                .and_then(|a| a.login.as_ref())
                                .unwrap_or(&empty_str)
                                .to_string();

                            let upvotes_str = match discussion.upvote_count {
                                Some(c) if c > 0 => format!("Upvotes: {}", c),
                                _ => "".to_string(),
                            };
                            let body_text = match discussion.body.as_ref() {
                                Some(text) => squeeze_fit_remove_quoted(&text, "```", 500, 0.6),
                                None => "".to_string(),
                            };
                            let mut disuccsion_texts = format!(
                                "Title: '{}' Url: '{}' Body: '{}' Created At: {} {} Author: {}\n",
                                title, url, body_text, date, upvotes_str, author_login
                            );

                            if let Some(comments) = &discussion.comments {
                                if let Some(ref edges) = comments.edges {
                                    for comment_edge_option in
                                        edges.iter().filter_map(|e| e.as_ref())
                                    {
                                        if let Some(comment) = &comment_edge_option.node {
                                            let stripped_comment_text = squeeze_fit_remove_quoted(
                                                &comment.body.as_ref().unwrap_or(&empty_str),
                                                "```",
                                                300,
                                                0.6,
                                            );
                                            let comment_author = comment
                                                .author
                                                .as_ref()
                                                .and_then(|a| a.login.as_ref())
                                                .unwrap_or(&empty_str);
                                            disuccsion_texts.push_str(
                                                &(format!(
                                                    "{comment_author} comments: '{stripped_comment_text}'\n")),
                                            );
                                        }
                                    }
                                }
                            }
                            let discussion_texts =
                                squeeze_fit_remove_quoted(&disuccsion_texts, "```", 9000, 0.4);
                            let target_str = match &target_person {
                                Some(person) => format!("{}'s", person),
                                None => "key participants'".to_string(),
                            };

                            let sys_prompt_1 = &format!(
                                    "Analyze the provided GitHub discussion. Identify the main topic, actions by participants, crucial viewpoints, solutions or consensus reached, and particularly highlight the contributions of specific individuals, especially '{target_str}'. Summarize without being verbose."
                                );

                            let co = match disuccsion_texts.len() > 12000 {
                                true => ChatOptions {
                                    model: chat::ChatModel::GPT35Turbo16K,
                                    system_prompt: Some(sys_prompt_1),
                                    restart: true,
                                    temperature: Some(0.7),
                                    max_tokens: Some(256),
                                    ..Default::default()
                                },
                                false => ChatOptions {
                                    model: chat::ChatModel::GPT35Turbo,
                                    system_prompt: Some(sys_prompt_1),
                                    restart: true,
                                    temperature: Some(0.7),
                                    max_tokens: Some(192),
                                    ..Default::default()
                                },
                            };

                            let usr_prompt_1 = &format!(
                                    "Analyze the content: {disuccsion_texts}. Briefly summarize the central topic, participants' actions, primary viewpoints, and outcomes. Emphasize the role of '{target_str}' in driving the discussion or reaching a resolution. Aim for a succinct summary that is rich in analysis and under 192 tokens."
                                );

                            match openai
                                .chat_completion("discussion99", usr_prompt_1, &co)
                                .await
                            {
                                Ok(r) => {
                                    text_out.push_str(&(format!("{} {}", url, r.choice)));
                                    git_mem_vec.push(GitMemory {
                                        memory_type: MemoryType::Discussion,
                                        name: author_login,
                                        tag_line: title,
                                        source_url: source_url,
                                        payload: r.choice,
                                        date: date,
                                    });
                                }

                                Err(_e) => log::error!(
                                    "Error generating discussion summary #{}: {}",
                                    url,
                                    _e
                                ),
                            }
                        }
                    }
                }
            }
        },
    }

    if git_mem_vec.is_empty() {
        None
    } else {
        Some((text_out, git_mem_vec))
    }
}
/* pub async fn search_discussions_integrated_chain(
    github_token: &str,
    search_query: &str,
    target_person: &Option<String>,
) -> Option<(String, Vec<GitMemory>)> {
    #[derive(Debug, Deserialize)]
    struct DiscussionRoot {
        data: Option<Data>,
    }

    #[derive(Debug, Deserialize)]
    struct Data {
        search: Option<Search>,
    }

    #[derive(Debug, Deserialize)]
    struct Search {
        edges: Option<Vec<Option<Edge>>>,
    }

    #[derive(Debug, Deserialize)]
    struct Edge {
        node: Option<Discussion>,
    }

    #[derive(Debug, Deserialize)]
    struct Discussion {
        title: Option<String>,
        url: Option<String>,
        author: Option<Author>,
        body: Option<String>,
        comments: Option<Comments>,
        #[serde(rename = "createdAt")]
        created_at: DateTime<Utc>,
        #[serde(rename = "upvoteCount")]
        upvote_count: Option<u32>,
    }

    #[derive(Debug, Deserialize)]
    struct Comments {
        edges: Option<Vec<Option<CommentEdge>>>,
    }

    #[derive(Debug, Deserialize)]
    struct CommentEdge {
        node: Option<CommentNode>,
    }

    #[derive(Debug, Deserialize)]
    struct CommentNode {
        author: Option<Author>,
        body: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct Author {
        login: Option<String>,
    }

    let base_url = "https://api.github.com/graphql";

    let query = format!(
        r#"
        query {{
            search(query: "{search_query}", type: DISCUSSION, first: 100) {{
                edges {{
                    node {{
                        ... on Discussion {{
                            title
                            url
                            body
                            author {{
                                login
                            }}
                            createdAt
                            upvoteCount
                            comments (first: 100) {{
                                edges {{
                                    node {{
                                        author {{
                                            login
                                        }}
                                        body
                                    }}
                                }}
                            }}
                        }}
                    }}
                }}
            }}
        }}
        "#,
        search_query = search_query
    );
    let mut git_mem_vec = Vec::with_capacity(100);
    let mut text_out = String::from("DISCUSSIONS \n");

    match github_http_post(&github_token, base_url, &query).await {
        None => {
            log::error!(
                "Failed to send the request to get DiscussionRoot: {}",
                base_url
            );
            return None;
        }
        Some(response) => match serde_json::from_slice::<DiscussionRoot>(&response) {
            Err(e) => {
                log::error!("Failed to parse the response for DiscussionRoot: {}", e);
                return None;
            }
            Ok(results) => {
                let empty_str = "".to_string();

                if let Some(search) = results.data?.search {
                    for edge_option in search.edges?.iter().filter_map(|e| e.as_ref()) {
                        if let Some(discussion) = &edge_option.node {
                            let date = discussion.created_at.date_naive();
                            let title = discussion.title.as_ref().unwrap_or(&empty_str).to_string();
                            let url = discussion.url.as_ref().unwrap_or(&empty_str).to_string();
                            let author_login = discussion
                                .author
                                .as_ref()
                                .and_then(|a| a.login.as_ref())
                                .unwrap_or(&empty_str)
                                .to_string();

                            let upvotes_str = match discussion.upvote_count {
                                Some(c) if c > 0 => format!("Upvotes: {}", c),
                                _ => "".to_string(),
                            };
                            let body_text = match discussion.body.as_ref() {
                                Some(text) => squeeze_fit_remove_quoted(&text, "```", 500, 0.6),
                                None => "".to_string(),
                            };
                            let mut disuccsion_texts = format!(
                                "Title: '{}' Url: '{}' Body: '{}' Created At: {} {} Author: {}\n",
                                title, url, body_text, date, upvotes_str, author_login
                            );

                            if let Some(comments) = &discussion.comments {
                                if let Some(ref edges) = comments.edges {
                                    for comment_edge_option in
                                        edges.iter().filter_map(|e| e.as_ref())
                                    {
                                        if let Some(comment) = &comment_edge_option.node {
                                            let comment_texts = format!(
                                                "{} comments: '{}'\n",
                                                comment
                                                    .author
                                                    .as_ref()
                                                    .and_then(|a| a.login.as_ref())
                                                    .unwrap_or(&empty_str),
                                                comment.body.as_ref().unwrap_or(&empty_str)
                                            );

                                            let stripped_comment_text = squeeze_fit_remove_quoted(
                                                &comment_texts,
                                                "```",
                                                300,
                                                0.6,
                                            );
                                            disuccsion_texts.push_str(&stripped_comment_text);
                                        }
                                    }
                                }
                            }
                            let discussion_texts =
                                squeeze_fit_remove_quoted(&disuccsion_texts, "```", 6000, 0.4);
                            let target_str = match &target_person {
                                Some(person) => format!("{}'s", person),
                                None => "key participants'".to_string(),
                            };
                            let sys_prompt_1 =
                                "Given the information on a GitHub discussion, your task is to analyze the content of the discussion posts. Extract key details including the main topic or question raised, any steps taken by the original author and commenters to address the problem, relevant discussions, and any identified solutions, consensus reached, or pending tasks.";

                            let usr_prompt_1 = &format!(
                                    "Based on the GitHub discussion post: {}, please list the following key details: The main topic or question raised in the discussion. Any steps or actions taken by the original author or commenters to address the discussion. Key discussions or points of view shared by participants in the discussion thread. Any solutions identified, consensus reached, or pending tasks if the discussion hasn't been resolved. The role and contribution of the user or commenters in the discussion.",
                                    disuccsion_texts
                                );

                            let usr_prompt_2 = &format!(
                                    "Provide a brief summary highlighting the core topic and emphasize the overarching contribution made by '{target_str}' to the resolution of this discussion, ensuring your response stays under 128 tokens."
                                );

                            match chain_of_chat(
                                sys_prompt_1,
                                usr_prompt_1,
                                "discussion99",
                                256,
                                usr_prompt_2,
                                128,
                                &format!("Error generating discussion summary #{}", url),
                            )
                            .await
                            {
                                Some(summary) => {
                                    text_out.push_str(&(format!("{} {}", url, summary)));
                                    git_mem_vec.push(GitMemory {
                                        memory_type: MemoryType::Discussion,
                                        name: author_login,
                                        tag_line: title,
                                        source_url: url,
                                        payload: summary,
                                        date: date,
                                    });
                                }

                                None => log::error!("Error generating discussion summary #{}", url),
                            }
                        }
                    }
                }
            }
        },
    }

    if git_mem_vec.is_empty() {
        None
    } else {
        Some((text_out, git_mem_vec))
    }
} */

pub async fn search_users(github_token: &str, search_query: &str) -> Option<String> {
    #[derive(Debug, Deserialize)]
    struct User {
        name: Option<String>,
        login: Option<String>,
        url: Option<String>,
        #[serde(rename = "twitterUsername")]
        twitter_username: Option<String>,
        bio: Option<String>,
        company: Option<String>,
        location: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: Option<DateTime<Utc>>,
        email: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct UserNode {
        node: Option<User>,
    }

    #[derive(Debug, Deserialize)]
    struct UserEdge {
        edges: Option<Vec<Option<UserNode>>>,
    }

    #[derive(Debug, Deserialize)]
    struct UserSearch {
        search: Option<UserEdge>,
    }

    #[derive(Debug, Deserialize)]
    struct UserRoot {
        data: Option<UserSearch>,
    }

    let base_url = "https://api.github.com/graphql";
    let mut out = String::from("USERS: \n");

    let query = format!(
        r#"
        query {{
            search(query: "{search_query}", type: USER, first: 100) {{
                edges {{
                    node {{
                        ... on User {{
                            name
                            login
                            url
                            twitterUsername
                            bio
                            company
                            location
                            createdAt
                            email
                        }}
                    }}
                }}
            }}
        }}
        "#,
        search_query = search_query
    );

    match github_http_post(&github_token, base_url, &query).await {
        None => {
            log::error!("Failed to send the request to get UserRoot: {}", base_url);
            return None;
        }
        Some(res) => match serde_json::from_slice::<UserRoot>(res.as_slice()) {
            Err(e) => {
                log::error!("Failed to parse the response for UserRoot: {}", e);
                return None;
            }
            Ok(results) => {
                if let Some(search) = &results.data {
                    if let Some(edges) = &search.search {
                        for edge_option in edges.edges.as_ref().unwrap_or(&vec![]) {
                            if let Some(edge) = edge_option {
                                if let Some(user) = &edge.node {
                                    let login_str = match &user.login {
                                        Some(login) => format!("Login: {},", login),
                                        None => {
                                            continue;
                                        }
                                    };
                                    let name_str = match &user.name {
                                        Some(name) => format!("Name: {},", name),
                                        None => String::new(),
                                    };

                                    let url_str = match &user.url {
                                        Some(url) => format!("Url: {},", url),
                                        None => String::new(),
                                    };

                                    let twitter_str = match &user.twitter_username {
                                        Some(twitter) => format!("Twitter: {},", twitter),
                                        None => String::new(),
                                    };

                                    let bio_str = match &user.bio {
                                        Some(bio) => format!("Bio: {},", bio),
                                        None => String::new(),
                                    };

                                    let company_str = match &user.company {
                                        Some(company) => format!("Company: {},", company),
                                        None => String::new(),
                                    };

                                    let location_str = match &user.location {
                                        Some(location) => format!("Location: {},", location),
                                        None => String::new(),
                                    };

                                    let date_str = match &user.created_at {
                                        Some(date) => {
                                            format!(
                                                "Created At: {},",
                                                date.date_naive().to_string()
                                            )
                                        }
                                        None => String::new(),
                                    };

                                    let email_str = match &user.email {
                                        Some(email) => format!("Email: {}", email),
                                        None => String::new(),
                                    };

                                    out.push_str(
                                            &format!(
                                                "{name_str} {login_str} {url_str} {twitter_str} {bio_str} {company_str} {location_str} {date_str} {email_str}\n"
                                            )
                                        );
                                }
                            }
                        }
                    }
                }
            }
        },
    }

    Some(out)
}

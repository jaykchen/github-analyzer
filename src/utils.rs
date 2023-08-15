use http_req::{request::Method, request::Request, response, uri::Uri};
use log;
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use serde_json::Value;
use std::collections::HashSet;
use store_flows::{get, set};

/*
use crypto::{symmetriccipher, buffer, aes, blockmodes};
use crypto::buffer::{ReadBuffer, WriteBuffer, BufferResult};
use rand::Rng;

use std::str;
use hex::{encode, decode};
use brotli::{BrotliCompress, BrotliDecompress};
use std::io::{Read, Write};

fn gen_key(url: &str, username: &str) -> String {
    let combined = format!("{}+{}", url, username);
    let encrypted_bytes = encrypt(combined.as_bytes(), KEY, IV).expect("Failed to encrypt");
    hex::encode(&encrypted_bytes)
}

fn get_vals(hex_key: &str) -> (String, String) {
    let encrypted_bytes = hex::decode(&hex_key).expect("Failed to decode hex");
    let decrypted_bytes = decrypt(&encrypted_bytes, KEY, IV).expect("Failed to decrypt");

    let decrypted_str = str::from_utf8(&decrypted_bytes).unwrap();
    let parts: Vec<&str> = decrypted_str.split('+').collect();
    (parts[0].to_string(), parts[1].to_string())
}



 */

pub fn squeeze_fit_commits_issues(commits: &str, issues: &str, split: f32) -> (String, String) {
    let mut commits_vec = commits.split_whitespace().collect::<Vec<&str>>();
    let commits_len = commits_vec.len();
    let mut issues_vec = issues.split_whitespace().collect::<Vec<&str>>();
    let issues_len = issues_vec.len();

    if commits_len + issues_len > 44_000 {
        let commits_to_take = (44_000 as f32 * split) as usize;
        match commits_len > commits_to_take {
            true => commits_vec.truncate(commits_to_take),
            false => {
                let issues_to_take = 44_000 - commits_len;
                issues_vec.truncate(issues_to_take);
            }
        }
    }
    (commits_vec.join(" "), issues_vec.join(" "))
}

pub fn squeeze_fit_remove_quoted(
    inp_str: &str,
    quote_mark: &str,
    max_len: u16,
    split: f32,
) -> String {
    let mut body = String::new();
    let mut inside_quote = false;
    let max_len = max_len as usize;

    for line in inp_str.lines() {
        if line.contains(quote_mark) {
            inside_quote = !inside_quote;
            continue;
        }

        if !inside_quote {
            body.push_str(line);
            body.push('\n');
        }
    }

    let body_len = body.split_whitespace().count();
    let n_take_from_beginning = (max_len as f32 * split) as usize;
    let n_keep_till_end = max_len - n_take_from_beginning;
    match body_len > max_len {
        false => body,
        true => {
            let mut body_text_vec = body.split_whitespace().collect::<Vec<&str>>();
            let drain_to = std::cmp::min(body_len, max_len);
            body_text_vec.drain(n_take_from_beginning..drain_to - n_keep_till_end);
            body_text_vec.join(" ")
        }
    }
}

pub async fn chain_of_chat(
    sys_prompt_1: &str,
    usr_prompt_1: &str,
    chat_id: &str,
    gen_len_1: u16,
    usr_prompt_2: &str,
    gen_len_2: u16,
    error_tag: &str,
) -> Option<String> {
    let openai = OpenAIFlows::new();

    let co_1 = ChatOptions {
        model: ChatModel::GPT35Turbo16K,
        restart: true,
        system_prompt: Some(sys_prompt_1),
        max_tokens: Some(gen_len_1),
        temperature: Some(0.7),
        ..Default::default()
    };

    match openai.chat_completion(chat_id, usr_prompt_1, &co_1).await {
        Ok(res_1) => {
            let sys_prompt_2 = serde_json::json!([{"role": "system", "content": sys_prompt_1},
    {"role": "user", "content": usr_prompt_1},
    {"role": "assistant", "content": &res_1.choice}])
            .to_string();

            let co_2 = ChatOptions {
                model: ChatModel::GPT35Turbo16K,
                restart: false,
                system_prompt: Some(&sys_prompt_2),
                max_tokens: Some(gen_len_2),
                temperature: Some(0.7),
                ..Default::default()
            };
            match openai.chat_completion(chat_id, usr_prompt_2, &co_2).await {
                Ok(res_2) => {
                    if res_2.choice.len() < 10 {
                        log::error!(
                            "{}, GPT generation went sideway: {:?}",
                            error_tag,
                            res_2.choice
                        );
                        return None;
                    }
                    return Some(res_2.choice);
                }
                Err(_e) => log::error!("{}, Step 2 GPT generation error {:?}", error_tag, _e),
            };
        }
        Err(_e) => log::error!("{}, Step 1 GPT generation error {:?}", error_tag, _e),
    }

    None
}

pub async fn github_http_fetch(token: &str, url: &str) -> Option<Vec<u8>> {
    let url = Uri::try_from(url).unwrap();
    let mut writer = Vec::new();

    match Request::new(&url)
        .method(Method::GET)
        .header("User-Agent", "flows-network connector")
        .header("Content-Type", "application/vnd.github.v3+json")
        .header("Authorization", &format!("Bearer {token}"))
        .send(&mut writer)
    {
        Ok(res) => {
            if !res.status_code().is_success() {
                log::error!("Github http error {:?}", res.status_code());
                return None;
            };

            Some(writer)
        }
        Err(_e) => {
            log::error!("Error getting response from Github: {:?}", _e);
            None
        }
    }
}

pub fn github_fetch_with_header(
    token: &str,
    url: &str,
) -> Result<(response::Response, Vec<u8>), Box<dyn std::error::Error>> {
    let uri = Uri::try_from(url)?;
    let mut writer = std::io::Cursor::new(Vec::new());

    let response = match Request::new(&uri)
        .method(Method::GET)
        .header("User-Agent", "flows-network connector")
        .header("Content-Type", "application/vnd.github.v3+json")
        .header("Authorization", &format!("Bearer {}", token))
        .send(&mut writer)
    {
        Ok(res) => {
            if !res.status_code().is_success() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "github_fetch_with_header encountered Github http error",
                )));
            };
            res
        }
        Err(e) => {
            log::error!("Error getting response from Github: {:?}", e);
            return Err(Box::new(e));
        }
    };

    Ok((response, writer.into_inner()))
}

pub async fn github_http_post(token: &str, base_url: &str, query: &str) -> Option<Vec<u8>> {
    let base_url = Uri::try_from(base_url).unwrap();
    let mut writer = Vec::new();

    let query = serde_json::json!({"query": query});
    match Request::new(&base_url)
        .method(Method::POST)
        .header("User-Agent", "flows-network connector")
        .header("Content-Type", "application/json")
        .header("Authorization", &format!("Bearer {}", token))
        .header("Content-Length", &query.to_string().len())
        .body(&query.to_string().into_bytes())
        .send(&mut writer)
    {
        Ok(res) => {
            if !res.status_code().is_success() {
                log::error!("Github http error {:?}", res.status_code());
                return None;
            };
            Some(writer)
        }
        Err(_e) => {
            log::error!("Error getting response from Github: {:?}", _e);
            None
        }
    }
}

pub async fn save_user(owner: &str, repo: &str, user_name: &str) -> bool {
    use std::hash::Hasher;
    use twox_hash::XxHash;
    let repo_string = format!("{owner}/{repo}");
    let mut hasher = XxHash::with_seed(0);
    hasher.write(repo_string.as_bytes());
    let hash = hasher.finish();
    let key = &format!("{:x}", hash);

    let mut existing_users: HashSet<String> = get(key)
        .and_then(|val| serde_json::from_value(val).ok())
        .unwrap_or_else(HashSet::new);

    // Check if the user_name already exists
    let already_exists = existing_users.contains(user_name);

    // If the user_name is not in the set, add it
    if !already_exists {
        existing_users.insert(user_name.to_string());
    }

    // Save updated records
    set(
        key,
        Value::String(serde_json::to_string(&existing_users).unwrap()),
        None,
    );

    // If the user_name was added, return true; otherwise, return false
    !already_exists
}

//! Public GitHub data for one login via a single GraphQL query with the server PAT.
//! Rank uses the github-readme-stats percentile formula.

use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct GithubData {
    pub name: String,
    pub stars: u64,
    pub commits: u64,
    pub prs: u64,
    pub issues: u64,
    pub followers: u64,
    /// (level, percentile) e.g. ("A+", 10.3)
    pub rank: (&'static str, f64),
    /// (name, colour, bytes), largest first, at most 6.
    pub languages: Vec<(String, String, u64)>,
    pub streak: Streak,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Streak {
    pub total: u64,
    pub current: u64,
    pub longest: u64,
}

const QUERY: &str = r#"query($login: String!) {
  user(login: $login) {
    name login
    followers { totalCount }
    pullRequests { totalCount }
    issues { totalCount }
    contributionsCollection {
      totalCommitContributions restrictedContributionsCount totalPullRequestReviewContributions
      contributionCalendar { totalContributions weeks { contributionDays { contributionCount } } }
    }
    repositories(first: 100, ownerAffiliations: OWNER, isFork: false, orderBy: {field: STARGAZERS, direction: DESC}) {
      nodes { stargazerCount languages(first: 10, orderBy: {field: SIZE, direction: DESC}) { edges { size node { name color } } } }
    }
  }
}"#;

pub async fn fetch(http: &reqwest::Client, token: &str, login: &str) -> Result<GithubData, String> {
    if token.is_empty() {
        return Err("GitHub not configured".into());
    }
    let body: Value = http
        .post("https://api.github.com/graphql")
        .bearer_auth(token)
        .header("User-Agent", "midtone-stats")
        .json(&json!({ "query": QUERY, "variables": { "login": login } }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    parse(&body)
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

fn parse(body: &Value) -> Result<GithubData, String> {
    let u = &body["data"]["user"];
    if u.is_null() {
        // Bad token / rate limit come back as `message` or `errors[0].message`.
        let msg = body["message"].as_str().or(body["errors"][0]["message"].as_str());
        tracing::warn!("github: {}", msg.unwrap_or("user not found"));
        return Err("GitHub user not found".into());
    }
    let cc = &u["contributionsCollection"];
    let repos = u["repositories"]["nodes"].as_array().cloned().unwrap_or_default();

    let mut langs: Vec<(String, String, u64)> = Vec::new();
    for edge in repos.iter().flat_map(|r| r["languages"]["edges"].as_array().cloned().unwrap_or_default()) {
        let name = edge["node"]["name"].as_str().unwrap_or("").to_string();
        let colour = edge["node"]["color"].as_str().unwrap_or("#858585").to_string();
        match langs.iter_mut().find(|l| l.0 == name) {
            Some(l) => l.2 += n(&edge["size"]),
            None => langs.push((name, colour, n(&edge["size"]))),
        }
    }
    langs.sort_by_key(|l| std::cmp::Reverse(l.2));
    langs.truncate(6);

    // ponytail: calendar covers the last year only, so streaks are capped at ~365 days.
    let days: Vec<u64> = cc["contributionCalendar"]["weeks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|w| w["contributionDays"].as_array().cloned().unwrap_or_default())
        .map(|d| n(&d["contributionCount"]))
        .collect();

    let commits = n(&cc["totalCommitContributions"]) + n(&cc["restrictedContributionsCount"]);
    let prs = n(&u["pullRequests"]["totalCount"]);
    let issues = n(&u["issues"]["totalCount"]);
    let reviews = n(&cc["totalPullRequestReviewContributions"]);
    let stars = repos.iter().map(|r| n(&r["stargazerCount"])).sum();
    let followers = n(&u["followers"]["totalCount"]);
    Ok(GithubData {
        name: u["name"].as_str().filter(|s| !s.is_empty()).or(u["login"].as_str()).unwrap_or("").to_string(),
        stars,
        commits,
        prs,
        issues,
        followers,
        rank: rank(commits, prs, issues, reviews, stars, followers),
        languages: langs,
        streak: streak(&days, n(&cc["contributionCalendar"]["totalContributions"])),
    })
}

/// `days` is oldest-first and ends today. Today having no contributions yet does not break the streak.
pub fn streak(days: &[u64], total: u64) -> Streak {
    let mut longest = 0;
    let mut run = 0;
    for &d in days {
        run = if d > 0 { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let skip_today = usize::from(days.last() == Some(&0));
    let current = days.iter().rev().skip(skip_today).take_while(|&&d| d > 0).count() as u64;
    Streak { total, current, longest }
}

pub fn rank(commits: u64, prs: u64, issues: u64, reviews: u64, stars: u64, followers: u64) -> (&'static str, f64) {
    let exp = |x: f64| 1.0 - 2f64.powf(-x);
    let log = |x: f64| x / (1.0 + x);
    let (cw, pw, iw, rw, sw, fw) = (2.0, 3.0, 1.0, 1.0, 4.0, 1.0);
    let score = cw * exp(commits as f64 / 250.0)
        + pw * exp(prs as f64 / 50.0)
        + iw * exp(issues as f64 / 25.0)
        + rw * exp(reviews as f64 / 2.0)
        + sw * log(stars as f64 / 50.0)
        + fw * log(followers as f64 / 10.0);
    let pct = (1.0 - score / (cw + pw + iw + rw + sw + fw)) * 100.0;
    const LEVELS: [(f64, &str); 9] = [
        (1.0, "S"), (12.5, "A+"), (25.0, "A"), (37.5, "A-"), (50.0, "B+"),
        (62.5, "B"), (75.0, "B-"), (87.5, "C+"), (100.0, "C"),
    ];
    let level = LEVELS.iter().find(|(t, _)| pct <= *t).map_or("C", |l| l.1);
    (level, pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaks() {
        assert_eq!(streak(&[1, 1, 0, 1, 1, 1, 0], 6), Streak { total: 6, current: 3, longest: 3 });
        assert_eq!(streak(&[1, 0, 2, 2], 5).current, 2);
        assert_eq!(streak(&[1, 0, 0], 1).current, 0);
        assert_eq!(streak(&[], 0), Streak { total: 0, current: 0, longest: 0 });
    }

    #[test]
    fn ranks() {
        assert_eq!(rank(0, 0, 0, 0, 0, 0), ("C", 100.0));
        assert_eq!(rank(100_000, 10_000, 10_000, 10_000, 1_000_000, 1_000_000).0, "S");
        // github-readme-stats reference: a mid user lands around B/B+
        let (level, _) = rank(250, 50, 25, 2, 50, 10);
        assert!(["B+", "B", "A-"].contains(&level), "{level}");
    }

    #[test]
    fn parses_and_merges_languages() {
        let body = json!({ "data": { "user": {
            "name": "", "login": "octo",
            "followers": { "totalCount": 3 }, "pullRequests": { "totalCount": 4 }, "issues": { "totalCount": 5 },
            "contributionsCollection": {
                "totalCommitContributions": 10, "restrictedContributionsCount": 2, "totalPullRequestReviewContributions": 1,
                "contributionCalendar": { "totalContributions": 7, "weeks": [{ "contributionDays": [
                    { "contributionCount": 1 }, { "contributionCount": 2 }] }] }
            },
            "repositories": { "nodes": [
                { "stargazerCount": 5, "languages": { "edges": [{ "size": 10, "node": { "name": "Rust", "color": "#dea584" } }] } },
                { "stargazerCount": 1, "languages": { "edges": [
                    { "size": 30, "node": { "name": "Go", "color": null } },
                    { "size": 25, "node": { "name": "Rust", "color": "#dea584" } }] } }
            ] }
        } } });
        let d = parse(&body).unwrap();
        assert_eq!(d.name, "octo");
        assert_eq!((d.stars, d.commits, d.prs), (6, 12, 4));
        assert_eq!(d.languages[0], ("Rust".into(), "#dea584".into(), 35));
        assert_eq!(d.languages[1].1, "#858585");
        assert_eq!(d.streak.current, 2);
        assert!(parse(&json!({ "data": { "user": null } })).is_err());
    }
}

#!/usr/bin/env python

import os
import sys
import toml
from github import Github
from pathlib import Path

def has_breaking_change(commits):
    for commit in commits:
        commit_message = commit.commit.message
        if '!' in commit_message.split('\n')[0] or 'BREAKING CHANGE' in commit_message:
            return True
    return False


def read_pr_numbers(file_path):
    with open(file_path, 'r') as file:
        return [int(line.strip()) for line in file]


def get_crate_version(crate_name):
    cargo_toml_path = Path(f"{crate_name}/Cargo.toml")
    if not cargo_toml_path.exists():
        raise FileNotFoundError(f"Cargo.toml not found for crate {crate_name}")
    
    with open(cargo_toml_path, 'r') as f:
        cargo_toml = toml.load(f)
    
    version = cargo_toml.get('package', {}).get('version')
    if not version:
        raise ValueError(f"Version not found in Cargo.toml for crate {crate_name}")
    return version


def get_pr_list(pr_numbers):
    token = os.getenv("GITHUB_PAT_SAFE_NETWORK_PR_LIST")
    if not token:
        raise Exception("The GITHUB_PAT_SAFE_NETWORK_PR_LIST environment variable must be set")

    g = Github(token)
    repo = g.get_repo("maidsafe/autonomi")

    filtered_pulls = []
    for pr_num in pr_numbers:
        print(f"Processing #{pr_num}...")
        pull = repo.get_pull(pr_num)
        if not pull.closed_at and not pull.merged_at:
            raise Exception(f"PR {pr_num} has not been closed yet")
        commits = pull.get_commits()
        breaking = has_breaking_change(commits)
        filtered_pulls.append({
            "number": pull.number,
            "title": pull.title,
            "author": pull.user.login,
            "closed_at": pull.closed_at,
            "breaking": breaking,
            "commits": commits
        })
    filtered_pulls.sort(key=lambda pr: pr["closed_at"])

    markdown_lines = []
    for pr in filtered_pulls:
        pr_number = pr["number"]
        closed_date = pr["closed_at"].date()
        breaking_text = "[BREAKING]" if pr["breaking"] else ""
        markdown_lines.append(f"{closed_date} [#{pr_number}](https://github.com/maidsafe/autonomi/pull/{pr_number}) -- {pr['title']} [@{pr['author']}] {breaking_text}")
    return markdown_lines


def main(pr_numbers):
    crate_binary_map = {
        "ant-node": "antnode",
        "ant-node-manager": "antctl",
        "ant-cli": "ant",
        "nat-detection": "nat-detection",
        "node-launchpad": "node-launchpad"
    }

    markdown_doc = []
    markdown_doc.append("## Binary Versions\n")
    for crate, binary in crate_binary_map.items():
        version = get_crate_version(crate)
        markdown_doc.append(f"* `{binary}`: v{version}")
    
    markdown_doc.append("\n## Merged Pull Requests\n")
    markdown_doc.extend(get_pr_list(pr_numbers))

    markdown_doc.append("\n## Detailed Changes\n")

    markdown_doc = "\n".join(markdown_doc)
    print()
    print(markdown_doc)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: python script.py <file_path>")
        sys.exit(1)
    
    file_path = sys.argv[1]
    main(read_pr_numbers(file_path))

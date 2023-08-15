# GitHub Analyzer

## Overview
This Discord bot provides a seamless integration with GitHub to present a weekly summary of activities in a specified repository. It offers insights into contributions, focusing on commits, issues, and discussions within a given timeframe.

## Features
- **User-Specific Reporting**: You can either focus on a particular GitHub user or gain insights into the contributions of prominent community members.
- **Versatile Data Retrieval**: The bot adeptly pulls and presents details pertaining to commits, issues, and discussions.

## Usage
**Slash Commands**: Activate the bot with /weekly-report [owner] [repo]. 
By calling the `handle_weekly_report` function and providing the repository's owner, repo name, and an optional GitHub username, you receive an exhaustive breakdown of the week's contributions. The bot ensures that the community is consistently updated on the latest repository advancements. If a username isn't supplied, the bot will default to the repository owner's perspective.

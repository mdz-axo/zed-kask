# hkask-mcp-research

Web search, extraction, and feed-based research MCP server.

## Tools (17)

| Tool | Description |
|------|-------------|
| `web_ping` | Liveness and provider health check |
| `web_search` | Search the web with RRF fusion across providers. Strategy selects providers: quick (single keyword), web (all), news (news-capable), deep (all + rerank) |
| `web_find_similar` | Find pages similar to a given URL using Exa findSimilar |
| `web_extract` | Extract content from a URL into markdown or structured JSON |
| `web_browse` | Interactive browsing of JS-heavy pages via headless browser |
| `rss_subscribe` | Subscribe to an RSS/Atom feed (Google Reader stream model) |
| `rss_unsubscribe` | Unsubscribe from a feed (stream_id e.g. 'feed/http://...') |
| `rss_list_subscriptions` | List subscriptions, optionally filtered by folder |
| `rss_fetch` | Fetch/sync new entries from a feed (supports ETag/Last-Modified) |
| `rss_get_entries` | Get entries from a stream (Google Reader stream IDs: feed/*, user/-/state/*, user/-/label/*) |
| `rss_mark_all_read` | Mark all entries in a stream as read |
| `rss_get_unread_count` | Get unread count for a stream |
| `rss_search` | Full-text search across feed entries |
| `rss_export_opml` | Export subscriptions as OPML 2.0 |
| `rss_import_opml` | Import subscriptions from OPML content |
| `rss_discover_feeds` | Discover RSS/Atom feeds from a URL via HTML link autodiscovery |
| `rss_edit_tag` | Edit tags on entries: mark read/unread, star/unstar, add/remove labels |

## Quick Start

```bash
# No API keys required for web search (uses free providers with fallback)
# The server starts automatically with kask
kask chat
# Or standalone:
hkask-mcp-research
```

## Usage

```
"Search for latest AI research papers"   → web_search
"Browse this URL and summarize it"        → web_browse
"Subscribe to the Rust blog RSS feed"     → rss_subscribe
"What's new in my feeds?"                → rss_get_entries
```

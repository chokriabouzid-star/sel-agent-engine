# Provider Routing & Fallback System

The `LiveProvider` (`src/llm/live.rs` and `src/llm/key_pool.rs`) manages communication with actual LLM inference endpoints. To ensure high availability and prevent rate limits from crashing long-running benchmarks, it implements a dynamic routing and fallback architecture.

## LLMProvider Trait

All models are abstracted behind the `LLMProvider` trait, which defines generic `chat` and `get_stats` methods. This allows `LiveProvider`, `ReplayProvider`, and `RecordProvider` to be used interchangeably.

## Key Pool & Limit Tracking

The engine loads multiple API keys from the environment (`GROQ_API_KEY`, `GEMINI_API_KEY`, `CEREBRAS_API_KEY`, etc.).
`key_pool.rs` and `limit_tracker.rs` work together to monitor API usage:
- If a provider responds with a `429 Too Many Requests`, a Quota Exceeded error, or an "exhausted" message, the key pool marks that specific provider/key combination as "Exhausted for today" or places it in a cool-down state.
- The Live Provider will automatically cycle to the next available API key or alternative provider model without crashing the agent.

## Dynamic Routing

Instead of hardcoding a single endpoint, the agent routes requests based on context:
1. It attempts the primary model requested via `SEL_MODEL`.
2. If the primary model's provider is exhausted or down, it falls back to a tier-list of secondary providers.
3. If all providers are exhausted, the agent gracefully fails with `All providers exhausted for today` rather than entering an infinite retry loop.

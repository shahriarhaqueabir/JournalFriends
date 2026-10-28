BEGIN;

-- OpenAI/llama.cpp tool-calling round-trip needs each assistant tool call and
-- its matching tool result to share an id. Without it the model cannot link a
-- result to the call, and replayed history loses tool context (the assistant
-- tool_calls and tool messages could not be reconstructed on the next turn).
ALTER TABLE ai_messages ADD COLUMN tool_call_id TEXT;

COMMIT;

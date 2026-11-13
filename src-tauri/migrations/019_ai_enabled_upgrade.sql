-- AI became opt-in in the same release that added `ai_enabled`. `seed_defaults`
-- runs after migrations on every launch and uses INSERT OR IGNORE, so on an
-- existing install it would silently seed `false` and switch off an AI setup
-- the user already had. Preserve that setup: when a chat model was configured
-- before this key existed, opt the install in. New installs have no
-- `llama_chat_model` row yet, so they still receive the `false` seed.
-- An explicit `ai_enabled` value (including a deliberate `false`) always wins.
INSERT INTO app_settings (key, value, updated_at)
SELECT 'ai_enabled', 'true', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE NOT EXISTS (
        SELECT 1 FROM app_settings WHERE key = 'ai_enabled'
      )
  AND EXISTS (
        SELECT 1 FROM app_settings
         WHERE key = 'llama_chat_model'
           AND TRIM(value) <> ''
      );

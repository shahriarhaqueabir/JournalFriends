-- GPU layers default: 'auto' lets llama.cpp size the offload to available VRAM
-- (--fit defaults to on). The legacy seed hard-pinned 99, which overrides both
-- and makes a model larger than free VRAM fail to load instead of partially
-- offloading. Only the seeded value is rewritten; a deliberate pin survives.
UPDATE app_settings
   SET value = 'auto',
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE key = 'llama_ngl'
   AND value = '99';

# Clear URLs

Removes common tracking query parameters from links in new messages before they are sent. Matching is case-insensitive and handles percent-encoded parameter names. Kept values, paths, fragments, surrounding text, and trailing punctuation are left unchanged.

The built-in patterns cover `utm`, `utm_*`, `ga_*`, `fbclid`, `gclid`, `dclid`, `msclkid`, `twclid`, `ttclid`, `yclid`, `gbraid`, `wbraid`, `mc_eid`, `mc_cid`, `mc_tc`, `mkt_tok`, `igshid`, `_ga`, `_gl`, `srsltid`, `ref_src`, `fb_source`, and `fb_ref`. The `si` parameter is also removed from `youtube.com` and `youtu.be` and their subdomains, including YouTube Music. It is kept on unrelated hosts. Generic parameters such as `ref`, `source`, and `s` are kept.

Everything runs locally: there are no downloads, cache, or network permissions. Previously saved plugin data is not removed. This is not a full provider rule or redirect engine, so it does not unwrap redirect links or remove provider-specific tracking outside the built-in rules.

Links inside code blocks are cleaned too. Message edits are not cleaned, only new messages. Messages are only changed when the result is not empty and fits in 4000 bytes.

Permission: `message-send`, to clean links in messages before sending.

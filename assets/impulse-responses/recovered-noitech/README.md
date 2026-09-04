# Recovered Noitech impulse responses

These WAV files are immutable source assets used by the recovered Noitech Bell
G through Bell M implementations. They are instrument-body responses, not room
reverb.

## `expensiveE.wav`

- Source: `Chadtech/BellsJobot`, repository root
- Original URL: <https://github.com/Chadtech/BellsJobot/blob/master/expensiveE.wav>
- Format: mono 16-bit PCM, 44.1 kHz, 303 frames
- SHA-256: `27618855a434a5398641528eb7cfc7af76032715526b94169aa826abf313873d`
- Used by Bell G, H, I, J, and K. Their source wet gains are `0.15`, `0.25`,
  `0.15`, `0.15`, and `0.15`, respectively.

## `home_clap_1.wav`

- Source: `Chadtech/Iconoclast`, `old-voices/home_clap_1.wav`
- Original URL: <https://github.com/Chadtech/Iconoclast/blob/master/old-voices/home_clap_1.wav>
- Format: mono 16-bit PCM, 44.1 kHz, 15,328 frames
- SHA-256: `33e4658f650193f0f85f043cfd786d2e3425dad00729bc01ec49180336566e5f`
- Used by Bell L and M with source wet gain `0.05`.

The historical builders first wrote the dry additive bell, ran `convolveMono`
with the response and factor above, then mixed that processed file back into
the dry bell. Ahess preserves that topology. At non-44.1-kHz output rates the
response is prepared with a band-limited synchronous resampler before playback.

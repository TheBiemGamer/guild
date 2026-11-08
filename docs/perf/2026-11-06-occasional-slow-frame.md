# Lead: occasional single slow frame when scrolling fast

Status: open lead, parked by the owner on 2026-11-06 (not a blocker).

## What was seen

After the ambient redesign (commits `873681c`, `f05ca91`), the `LAUNCHER_BENCH` autoscroll over the
real 613-game library sometimes shows **one** frame slower than 1.5× the median:

| Run                                     | 4000 px/s | 15000 px/s |
| --------------------------------------- | --------- | ---------- |
| Image logos (`f05ca91`), 3 + 4 runs     | 0, 0, 1   | 0, 1, 0, 0 |
| Inline SVG logos (`873681c`), final run | 0         | 1          |
| Logos removed (exploratory)             | 0 × 6     | 0 × 6      |

Each slow frame is a single missed vsync at 180 Hz (about 11.4 ms against a 5.7 ms median),
which isn't visible in normal use. The pre-redesign build had 0 in 10 of 10 runs, but those ran
with the old benchmark sampler.

## Why it's only a lead

- The old build was never re-measured with the new, quieter sampler in `scripts/bench.ps1`
  (process tree walked every 5 s instead of every 250 ms). The old sampler's CPU load may have
  hidden or caused misses on either side.
- The owner was using the PC during the runs, which can cause a missed frame by itself.

## Next steps when someone picks it up

Run these while nobody is using the PC (about 12 app launches in total):

1. Build the pre-redesign Tauri app (commit `9798330`, the last code commit before the redesign)
   and run it 4 times per speed with the current `scripts/bench.ps1`. If it also misses about 1 run in 4, it's
   noise: close this lead.
2. Only if the old build is clean: try one change at a time on the current code, 2 runs per speed
   each. Candidates: `contain: paint` on `.card`; removing the logo's `border` and dark circle
   (draw it as one pre-composed image); `will-change: transform` on `.slot`.

# Performance measurement

No current, verified comparison against Eclipse jdtls is shipped. This public
release makes no startup, latency, or memory guarantee. Cold requests and
large-workspace memory use remain areas to measure on representative projects.

Run measurements on a quiet machine with the documented JDK and dependencies.
Record the Hyperion revision and dirty status, hardware, power mode, commands,
workspace revision, cache state, and external artifact hashes. Keep raw
JSON, protocol traces, and generated reports together outside the source
snapshot. Do not select a best attempt or change a failing probe position to
improve the result. Every run, including failures and timeouts, belongs in the
evidence.

Cold and warm describe server-local workspace indexes, not the operating-system
page cache. Gradle daemons are excluded from direct server RSS. Latency and item counts
alone do not establish completion correctness.

For a regression, reproduce a fixed document and request sequence before and
after the change. Inspect diagnostics, definition positions, and rendered
results alongside timing. Separate startup, first-request, and warm-request
costs. A local synthetic measurement does not establish whole-project or
cross-server performance.

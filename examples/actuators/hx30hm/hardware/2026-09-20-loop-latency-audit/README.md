# Motor-loop latency investigation — September 20, 2026

This is an offline investigation of the retained, physically qualified September
15 three-motor FPGA image. It does not change firmware, flash an FPGA, command
motors, or establish a new hardware operating rate. The later streamed-reference
candidate remains separately uncommissioned.

## Physical evidence

The analysis covers all 1,000 complete frames in the five 10 ms qualification
captures. Durations use the 50 MHz FPGA event clock, not host arrival timestamps.
Every frame has three feedback transactions, one combined control transaction,
and three audit transactions; the accounting checks that durations plus gaps
equal the full transaction window.

| Measured quantity | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: |
| Complete transaction window | 2.937 ms | 3.127 ms | 3.645 ms |
| Sum of gaps between transactions | 1.092 ms | 1.281 ms | 1.807 ms |
| First feedback completion to final command byte | 0.898 ms | 0.898 ms | 1.609 ms |
| First-to-last feedback completion separation | 0.691 ms | 0.697 ms | 1.030 ms |

The previously discussed 1.780 ms gap total belongs to the longest **whole**
window, frame 0 of `motion-10ms-cap100-02`; it is not the maximum gap total or
the typical cycle. That frame contains 0.98022 ms reading, 0.17502 ms computing
and sending, 0.71034 ms auditing, and 1.77958 ms between transactions.
The first feedback completion to command completion is a transport-age metric,
not internal sensor age or mechanical response latency.

[`physical-summary.json`](physical-summary.json) retains precise statistics and
all seven transactions of the longest window. Original physical evidence is
unchanged in [`../2026-09-15-fast-loop`](../2026-09-15-fast-loop/README.md).

## What the firmware waits for

The UART to the motors and UART to the computer both run at 1 Mbaud in this
qualified image (`HOST_CLKS=50` in its build log, despite the source's 115200
default/comment). These are distinct UART links. Each 8N1 byte occupies 10 bits.

1. A motor request must finish, its reply must arrive, and the bridge must observe
   20 quiet bit times after the final received byte. That guard is about 20 us
   per read and protects bus ownership. The initial 2 ms reply timeout is a
   **maximum wait**, shortened when reply bytes arrive; it is not an unconditional
   2 ms sleep on every read.
2. The event encoder constructs a timestamped record. Copying 43–55 bytes into
   the output FIFO takes roughly 0.9–1.1 us at 50 MHz; that copy itself is small.
3. The bridge requires at least **128 free bytes** in its **255-byte usable**
   circular output FIFO before accepting another autonomous transaction. Existing
   bytes leave at about 10 us each. Waiting to recover 30 free bytes therefore
   costs about 0.30 ms. The motor bus can be idle during this wait.
4. Logging and queued host packets have explicit arbitration priority. Heartbeats
   can add occasional gaps. The experiment scheduler must also construct its next
   packet and obtain recorder credit. These mechanisms preserve packet ordering,
   raw error evidence, and lossless recording.

See the frozen [`bridge_control.v`](../2026-09-15-fast-loop/source/fpga/src/bridge_control.v),
especially its `experiment_packet_ready`, FIFO writer, and UART reader, and
[`experiment_event_packet.v`](../2026-09-15-fast-loop/source/fpga/src/experiment_event_packet.v).
The same 128-byte admission condition is still present in the later frozen
live-control candidate; the quantitative results here apply to the qualified
image, not that candidate.

## Sustained logging budget

For one successful three-motor frame:

| Output to the computer | Bytes |
| --- | ---: |
| Three forwarded raw feedback replies, 21 bytes each | 63 |
| Three forwarded raw audit replies, 12 bytes each | 36 |
| Three timestamped feedback records, 52 bytes each | 156 |
| One timestamped control record, including nine PWM slots | 55 |
| Three timestamped audit records, 43 bytes each | 129 |
| Total | **439** |

The raw replies and event records repeat successful motor payloads. Raw replies
also preserve malformed/partial/foreign traffic that the event payload alone
does not retain, so removing them blindly would weaken the evidence contract.

At 1 Mbaud, 439 bytes require **at least 4.39 ms** to transmit. Buffering and the
independent host UART allow this to overlap motor transactions and extend beyond
the reported 2.94–3.65 ms transaction window. The last audit's event still has to
drain after that window ends. A larger buffer can reduce intra-frame stalls but
cannot eliminate the long-run throughput requirement.

Thus **about 228 frames/s is an ideal host-output bandwidth ceiling for the
unchanged format**, before byte-handling overhead, heartbeats, start/terminal
records, and streamed-reference acknowledgements. It is not an accepted control
rate. The earlier reciprocal-of-3.65-ms estimate of 274 Hz overlooked this
sustained logging constraint and should not be used as a supported limit.

## Offline causal test

`investigate.mjs` copies the frozen production RTL into fresh temporary build
directories and reuses its complete UART/controller/supervisor fixture. It changes
only two fixture settings: mock reply turnaround from 1 ms to 30 us, and the host
UART divider between 50 (1 Mbaud) and 25 (2 Mbaud). Both cases retain the 1 Mbaud
motor link, 10 ms period, all seven transactions per frame, unchanged buffer and
guard thresholds, full-scale positive/negative synthetic commands, and complete
recording. No sensor-freshness or motor-physics model is introduced.

The observation-only `gap_monitor.vh` counts every inter-transaction clock, using
exclusive admission-gate priority: bus/ownership guard, local reply, record copy,
record encoder, queued host packet, FIFO reserve, then scheduler/packet handling.
One boundary clock per gap is reported separately and the script verifies exact
accounting. A clock can have more than one unavailable resource; these categories
attribute it to the first blocking gate, not independent additive causal effects.
Comparing the two otherwise identical simulations tests the host-drain bottleneck.

Both 25-frame runs passed the complete UART fixture, including checksum/packet
ordering, positive/negative full-scale commands, complete record counts, heartbeat
operation, and modeled final zero/off. The motor bus and loop period were unchanged.

| Synthetic metric | Computer link 1 Mbaud | Computer link 2 Mbaud |
| --- | ---: | ---: |
| Median complete transaction window | 2.92998 ms | 1.95168 ms |
| Median sum of inter-transaction gaps | 1.10334 ms | 0.12504 ms |
| Mean FIFO-reserve wait per frame | 0.99976 ms | 0.00341 ms |
| Mean bus/ownership-guard time in gaps | 0.11887 ms | 0.11887 ms |
| Mean event-copy time in gaps | 0.00594 ms | 0.00594 ms |
| Median first-feedback-to-command completion | 0.89786 ms | 0.87890 ms |
| Maximum complete transaction window | 3.83246 ms | 2.03746 ms |

The unchanged-link synthetic median (2.930 ms) closely matches the physical
median (2.937 ms), supporting this mechanism as an explanation. It does not make
the mock an exact physical replay: its fixed turnaround and immediate initial
heartbeat produce different startup maxima. Physical recordings do not contain
internal FIFO signals, so exact physical per-gate attribution remains unmeasured.

The controlled host-speed comparison removes **0.97830 ms (88.7%) of median gap
time**, while all measurement transactions remain. The mean guard time is
unchanged. This is strong offline evidence that host-output backpressure, rather
than motor turnaround or controller arithmetic, causes most of the excess gaps.

However, in an ordinary frame most removable waiting occurs **after** the drive
packet, before/between audits. Median feedback-to-command delay improves by only
**0.01896 ms**, not the entire 0.97830 ms saved from the frame. The immediate gain
is available cycle budget and less timing variation; a higher update cadence
would require a subsequent, separately validated controller/firmware change.

Results are retained in `simulation-summary.json`, CSV gap traces, complete UART
hex streams, and build/run logs. These are synthetic evidence, not a new physical
qualification. `provenance.json` hashes the input RTL, fixture, upload, monitor,
analysis script, and original physical event files.
`verify.mjs` independently checks all 27 input hashes, decodes/checksums all 366
output packets per run, and verifies that all 175 transaction records have
identical contents except timestamps and their derived checksums. Its result and
artifact hashes are in `verification.json`.

## Implication for policy fidelity

The next implementation should give control a bounded, predictable path while
retaining lossless, device-timestamped measurements. Priorities are:

1. Separate transaction/event capture from host serialization with a bounded
   queue and explicit overflow/cancellation behavior; preserve raw error evidence.
2. Batch/compact frame records so valid samples are not sent twice. Keep exact
   motor IDs, reference/frame identity, command bytes, request/completion times,
   audit results, and raw anomalous packets recoverable.
3. Qualify faster host transport if needed. A 2 Mbaud RTL result does not establish
   the actual USB bridge/driver's support or electrical reliability.
4. Measure deadline misses, command interval jitter, inter-motor sample skew, and
   feedback-to-command delay at 100 Hz and then 200 Hz, using the same physical
   reference and explicitly cadence-adjusted controller. Establish internal sensor
   refresh/age separately before claiming higher command rate gives new feedback.

Keeping per-frame audits in this investigation avoids obtaining an apparent
speedup by discarding checks. Larger buffers alone cannot sustain a producer rate
above the host link's capacity. Shorter transport delay and fresh, timestamped
feedback improve the policy-to-motor measurement contract; this work has not
measured improved tracking or mechanical bandwidth.

The later uncommissioned streamed-reference candidate also documents an 80–160 ms
reference queue. That is a separate policy-command latency issue, much larger than
these sub-frame gaps; it must not be confused with this pre-uploaded trajectory
benchmark or assumed to describe currently running hardware.

## Reproduce

From the repository root, with Node and Verilator/C++ available:

```sh
node examples/actuators/hx30hm/hardware/2026-09-20-loop-latency-audit/investigate.mjs
node examples/actuators/hx30hm/hardware/2026-09-20-loop-latency-audit/verify.mjs
```

This builds and runs only offline simulations. It never invokes flashing tools or
accesses a serial device. Simulation workspaces are separate temporary directories;
the script does not mutate original firmware or original physical captures.

# Open-source build for the servo lesson.
#
#   yosys -> nextpnr-himbaechel -> apicula (gowin_pack) -> openFPGALoader
#
# This is an alternative to build.tcl, which drives Gowin's own EDA. Nothing
# here needs a vendor account or a licence. Everything ships in oss-cad-suite:
#
#   https://github.com/YosysHQ/oss-cad-suite-build/releases
#
#   make          build impl/servo.fs
#   make flash    load it into SRAM (volatile -- gone at power-off)
#   make prog     write it to flash (survives power cycling)
#   make sim      run both testbenches
#   make clean

DEVICE := GW5A-LV25MG121NC1/I0
FAMILY := GW5A-25A
BOARD  := tangprimer25k
FREQ   := 50

SRC := src/uart_tx.v src/uart_rx.v src/link_rx.v src/hx_bus.v src/button.v \
       src/capture_stream.v src/link_tx.v src/servo.v
CST := src/servo.cst

# GW5A-25A REQUIRES the SSPI pins to be configured as GPIO, and nextpnr and
# gowin_pack cross-check each other on it: set it in one place only and the
# pack step aborts with "conflicting settings". Hence the flag twice.
SSPI := sspi_as_gpio

# The dock's LEDs sit on the DONE and READY config pins, so those have to be
# released to GPIO as well (same reasoning as build.tcl's set_option lines).
PACKFLAGS := --$(SSPI) --done_as_gpio --ready_as_gpio

all: impl/servo.fs

impl/servo.json: $(SRC)
	@mkdir -p impl
	yosys -p "read_verilog -I src $(SRC); synth_gowin -top top -json $@"

impl/servo_pnr.json: impl/servo.json $(CST)
	nextpnr-himbaechel --json $< --write $@ \
	  --device "$(DEVICE)" --vopt cst=$(CST) --vopt $(SSPI) --freq $(FREQ)

impl/servo.fs: impl/servo_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<

# SRAM load: fast, and forgotten at power-off. Use this while iterating --
# it means a bad bitstream is never more than a power cycle away from gone.
flash: impl/servo.fs
	openFPGALoader -b $(BOARD) $<

# Write to onboard flash: survives power cycling.
prog: impl/servo.fs
	openFPGALoader -b $(BOARD) -f $<

sim:
	@mkdir -p impl
	iverilog -g2005 -o impl/servo_tb.vvp tb/servo_tb.v tb/hx_model.v \
	  src/hx_bus.v src/uart_tx.v src/uart_rx.v && vvp impl/servo_tb.vvp
	iverilog -g2005 -I src -o impl/top_tb.vvp tb/top_tb.v tb/hx_model.v \
	  $(SRC) && vvp impl/top_tb.vvp
	iverilog -g2005 -I src -o impl/link_tb.vvp tb/link_tb.v \
	  src/link_tx.v src/link_rx.v src/uart_tx.v src/uart_rx.v && vvp impl/link_tb.vvp

# --- debug bridge -----------------------------------------------------------
# A separate top module: relays packets between the dock's USB-UART and the
# servo bus so tools/hx.py can interrogate the servo directly.
BSRC := src/uart_tx.v src/uart_rx.v src/bridge.v

bridge: impl/bridge.fs

impl/bridge.json: $(BSRC)
	@mkdir -p impl
	yosys -p "read_verilog $(BSRC); synth_gowin -top top -json $@"

impl/bridge_pnr.json: impl/bridge.json src/bridge.cst
	nextpnr-himbaechel --json $< --write $@ \
	  --device "$(DEVICE)" --vopt cst=src/bridge.cst --vopt $(SSPI) --freq $(FREQ)

impl/bridge.fs: impl/bridge_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<

bridge-flash: impl/bridge.fs
	openFPGALoader -b $(BOARD) $<

# Comparison image: electrically release G5 while waiting for servo replies.
impl/bridge_release.json: $(BSRC)
	@mkdir -p impl
	yosys -p "read_verilog $(BSRC); chparam -set RELEASE_TX 1 top; synth_gowin -top top -json $@"

impl/bridge_release_pnr.json: impl/bridge_release.json src/bridge.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" \
	  --vopt cst=src/bridge.cst --vopt $(SSPI) --freq $(FREQ)

impl/bridge_release.fs: impl/bridge_release_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<

bridge-release-flash: impl/bridge_release.fs
	openFPGALoader -b $(BOARD) $<

.PHONY: bridge-release-flash

# Temporary DC-low bus probe; restore normal UART with `make bridge-flash`.
impl/bus_level_probe.json: src/bus_level_probe.v
	@mkdir -p impl
	yosys -p "read_verilog $<; synth_gowin -top top -json $@"

impl/bus_level_probe_pnr.json: impl/bus_level_probe.json src/bus_level_probe.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" \
	  --vopt cst=src/bus_level_probe.cst --vopt $(SSPI) --freq $(FREQ)

impl/bus_level_probe.fs: impl/bus_level_probe_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<

bus-level-probe-flash: impl/bus_level_probe.fs
	openFPGALoader -b $(BOARD) $<

.PHONY: bus-level-probe-flash

# --- host link bitstream (tethered phase) ----------------------------------
LSRC := src/uart_tx.v src/uart_rx.v src/link_rx.v src/link_tx.v \
        src/link_core.v src/link_top.v

link: impl/link.fs

impl/link.json: $(LSRC)
	@mkdir -p impl
	yosys -p "read_verilog -I src $(LSRC); synth_gowin -top top -json $@"

impl/link_pnr.json: impl/link.json src/link.cst
	nextpnr-himbaechel --json $< --write $@ \
	  --device "$(DEVICE)" --vopt cst=src/link.cst --vopt $(SSPI) --freq $(FREQ)

impl/link.fs: impl/link_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<

link-flash: impl/link.fs
	openFPGALoader -b $(BOARD) $<

# --- verilator: closed-loop co-simulation ----------------------------------
# Drives the Verilated FPGA with the ESP32's OWN C codec (esp32/link_proto.c,
# compiled here exactly as it will be on the S3). Two independent
# implementations of one wire format, checked against each other.
VSRC := src/link_core.v src/link_rx.v src/link_tx.v src/uart_rx.v src/uart_tx.v
VPAR := -GCLKS_PER_BIT=25 -GTICK_DIV=20000 -GDEADMAN_CYC=200000

lint:
	verilator --lint-only -Wall -Isrc sim/verilator.vlt \
	  --top-module link_core $(VSRC)
	verilator --lint-only -Wall -Isrc sim/verilator.vlt \
	  --top-module top $(SRC)

sim-link:
	verilator --cc --exe --build -j 4 -Isrc sim/verilator.vlt \
	  --top-module link_core $(VPAR) --Mdir impl/vsim \
	  -CFLAGS "-I$(CURDIR)/../esp32 -O2" \
	  $(VSRC) sim/sim_main.cpp ../esp32/link_proto.c
	./impl/vsim/Vlink_core

clean:
	rm -rf impl *.vvp *.vcd

.PHONY: all flash prog sim clean bridge bridge-flash lint sim-link link link-flash capture sim-capture

# Timestamped acquisition profile; building does not program hardware.
capture: impl/capture.fs

impl/capture.json: $(SRC)
	@mkdir -p impl
	yosys -p "read_verilog -I src $(SRC); chparam -set CAPTURE_STREAM 1 -set HOST_CLKS_PER_BIT 50 -set POLL_CYC 50000 top; synth_gowin -nolutram -top top -json $@"

impl/capture_pnr.json: impl/capture.json $(CST)
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=$(CST) --vopt $(SSPI) --freq $(FREQ)

impl/capture.fs: impl/capture_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<

sim-capture:
	@mkdir -p impl
	iverilog -g2005 -I src -o impl/capture_tb.vvp tb/capture_tb.v src/capture_stream.v src/link_tx.v src/link_rx.v src/uart_tx.v src/uart_rx.v
	vvp impl/capture_tb.vvp
	iverilog -g2005 -I src -P top_tb.CAPTURE=1 -o impl/top_capture_tb.vvp tb/top_tb.v tb/hx_model.v $(SRC)
	vvp impl/top_capture_tb.vvp

# Supervised PWM bench profile. Build/test targets never access hardware.
SSRC := $(BSRC) src/hx_safety.v src/button.v
bridge-safety: impl/bridge_safety.fs
impl/bridge_safety.json: $(SSRC)
	@mkdir -p impl
	yosys -p "read_verilog $(SSRC); chparam -set RELEASE_TX 1 -set SAFETY_ENABLE 1 top; synth_gowin -nolutram -top top -json $@"
impl/bridge_safety_pnr.json: impl/bridge_safety.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" \
	  --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_safety.fs: impl/bridge_safety_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
sim-safety:
	@mkdir -p impl
	iverilog -g2005 -s hx_safety_tb -o impl/hx_safety_tb.vvp tb/hx_safety_tb.v src/hx_safety.v
	vvp impl/hx_safety_tb.vvp
	iverilog -g2005 -s bridge_safety_tb -o impl/bridge_safety_tb.vvp tb/bridge_safety_tb.v $(SSRC)
	vvp impl/bridge_safety_tb.vvp
	iverilog -g2005 -s bridge_release_tb -o impl/bridge_release_tb.vvp tb/bridge_release_tb.v $(BSRC)
	vvp impl/bridge_release_tb.vvp
.PHONY: bridge-safety sim-safety

# FPGA arithmetic controller, isolated from the retained supervised bridge image.
CSRC := src/uart_tx.v src/uart_rx.v src/host_packet_queue.v src/trajectory_store.v src/trajectory_upload.v src/bridge_control.v src/hx_safety.v src/button.v impl/fixed-pd/fixed_pd.v
bridge-control: impl/bridge_control.fs
impl/bridge_control.json: $(CSRC)
	yosys -p "read_verilog $(CSRC); chparam -set RELEASE_TX 1 -set SAFETY_ENABLE 1 top; synth_gowin -nodsp -nolutram -top top -json $@"
impl/bridge_control_pnr.json: impl/bridge_control.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_control.fs: impl/bridge_control_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
sim-control: sim-safety-index sim-safety-sync-scope
	iverilog -g2005 -s fixed_pd_tb -o impl/fixed-pd/parity.vvp impl/fixed-pd/fixed_pd_tb.v impl/fixed-pd/fixed_pd.v
	vvp impl/fixed-pd/parity.vvp
	iverilog -g2005 -s bridge_safety_tb -o impl/control_regression.vvp tb/bridge_safety_tb.v $(CSRC)
	vvp impl/control_regression.vvp
	iverilog -g2005 -s bridge_control_tb -o impl/control.vvp tb/bridge_control_tb.v $(CSRC)
	vvp impl/control.vvp
.PHONY: bridge-control sim-control

# Device-clock execution profile. The host link is 1 Mbaud; full duty is accepted
# only from the sealed onboard experiment. Build never programs hardware.
ESRC := $(CSRC) src/live_target_queue.v src/experiment_session.v src/experiment_transactions.v \
        src/experiment_scheduler.v src/experiment_packet.v src/experiment_reply.v \
        src/experiment_event_stream.v src/experiment_event_packet.v
bridge-experiment: impl/bridge_experiment.fs
impl/bridge_experiment.json: $(ESRC) Makefile
	yosys -p "read_verilog $(ESRC); chparam -set HOST_CLKS 50 -set RELEASE_TX 1 -set SAFETY_ENABLE 1 -set BUFFER_HOST 1 -set TRAJECTORY_ENABLE 1 -set EXPERIMENT_ENABLE 1 -set FIXED_GAINS 1 top; synth_gowin -family gw5a -strict-gw5a-dffs -nodsp -nolutram -nowidelut -top top -json $@"
impl/bridge_experiment_pnr.json: impl/bridge_experiment.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_experiment.fs: impl/bridge_experiment_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
.PHONY: bridge-experiment
sim-bridge-experiment:
	@mkdir -p impl
	iverilog -g2005 -s bridge_experiment_tb -P bridge_experiment_tb.CASE=$(CASE) -o impl/bridge_experiment_case$(CASE).vvp tb/bridge_experiment_tb.v $(ESRC)
	vvp impl/bridge_experiment_case$(CASE).vvp
.PHONY: sim-bridge-experiment
sim-bridge-experiment-fast:
	verilator --language 1364-2005 --binary --timing -j 4 -Wno-fatal --top-module bridge_experiment_tb -GCASE=$(CASE) --Mdir impl/vexperiment_case$(CASE) tb/bridge_experiment_tb.v $(ESRC)
	./impl/vexperiment_case$(CASE)/Vbridge_experiment_tb
.PHONY: sim-bridge-experiment-fast

# Autonomous experiment scheduler core: isolated protocol/arbitration contract.
# Not yet wired into bridge_control; these targets never program a board.
sim-experiment-scheduler:
	@mkdir -p impl
	iverilog -g2005 -s experiment_scheduler_tb -o impl/experiment_scheduler_tb.vvp tb/experiment_scheduler_tb.v src/experiment_scheduler.v
	vvp impl/experiment_scheduler_tb.vvp
check-experiment-scheduler:
	@mkdir -p impl
	yosys -Q -p "read_verilog src/experiment_scheduler.v; synth_gowin -nodsp -nolutram -top experiment_scheduler -json impl/experiment_scheduler.json; stat"
.PHONY: sim-experiment-scheduler check-experiment-scheduler

# Buffered host path needed before autonomous packet arbitration is connected.
sim-buffered-control:
	@mkdir -p impl
	iverilog -g2005 -s host_packet_queue_tb -o impl/host_packet_queue_tb.vvp tb/host_packet_queue_tb.v src/host_packet_queue.v
	vvp impl/host_packet_queue_tb.vvp
	iverilog -g2005 -DBUFFERED_CONTROL -s bridge_safety_tb -o impl/buffered_safety_tb.vvp tb/bridge_safety_tb.v $(CSRC)
	vvp impl/buffered_safety_tb.vvp
	iverilog -g2005 -s bridge_control_tb -P bridge_control_tb.BUFFERED=1 -o impl/buffered_control_tb.vvp tb/bridge_control_tb.v $(CSRC)
	vvp impl/buffered_control_tb.vvp
.PHONY: sim-buffered-control

# Build the buffered bridge separately; scheduling remains disconnected for now.
bridge-buffered: impl/bridge_buffered.fs
impl/bridge_buffered.json: $(CSRC)
	yosys -p "read_verilog $(CSRC); chparam -set RELEASE_TX 1 -set SAFETY_ENABLE 1 -set BUFFER_HOST 1 top; synth_gowin -nodsp -nolutram -top top -json $@"
impl/bridge_buffered_pnr.json: impl/bridge_buffered.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_buffered.fs: impl/bridge_buffered_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
.PHONY: bridge-buffered

# Prototype immutable trajectory RAM; does not program or run motors.
sim-trajectory-store:
	@mkdir -p impl
	iverilog -g2005 -s trajectory_store_tb -o impl/trajectory_store_tb.vvp tb/trajectory_store_tb.v src/trajectory_store.v
	vvp impl/trajectory_store_tb.vvp
check-trajectory-store:
	@mkdir -p impl
	yosys -Q -p "read_verilog src/trajectory_store.v; synth_gowin -nodsp -nolutram -top trajectory_store -json impl/trajectory_store.json; stat"
.PHONY: sim-trajectory-store check-trajectory-store
sim-trajectory-upload:
	@mkdir -p impl
	iverilog -g2005 -s trajectory_upload_tb -o impl/trajectory_upload_tb.vvp tb/trajectory_upload_tb.v src/trajectory_upload.v src/trajectory_store.v src/host_packet_queue.v
	vvp impl/trajectory_upload_tb.vvp
check-trajectory-upload:
	@mkdir -p impl
	yosys -Q -p "read_verilog src/trajectory_upload.v src/trajectory_store.v; synth_gowin -nodsp -nolutram -top trajectory_upload -json impl/trajectory_upload.json; stat"
.PHONY: sim-trajectory-upload check-trajectory-upload

# Upload-enabled buffered bridge. No autonomous START or motor scheduler yet.
sim-bridge-trajectory:
	@mkdir -p impl
	iverilog -g2005 -s bridge_trajectory_tb -o impl/bridge_trajectory_tb.vvp tb/bridge_trajectory_tb.v $(CSRC)
	vvp impl/bridge_trajectory_tb.vvp
	iverilog -g2005 -s bridge_control_tb -P bridge_control_tb.BUFFERED=1 -P bridge_control_tb.TRAJECTORY=1 -o impl/trajectory_control_tb.vvp tb/bridge_control_tb.v $(CSRC)
	vvp impl/trajectory_control_tb.vvp
bridge-trajectory: impl/bridge_trajectory.fs
impl/bridge_trajectory.json: $(CSRC)
	yosys -p "read_verilog $(CSRC); chparam -set RELEASE_TX 1 -set SAFETY_ENABLE 1 -set BUFFER_HOST 1 -set TRAJECTORY_ENABLE 1 top; synth_gowin -nodsp -nolutram -top top -json $@"
impl/bridge_trajectory_pnr.json: impl/bridge_trajectory.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_trajectory.fs: impl/bridge_trajectory_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
.PHONY: sim-bridge-trajectory bridge-trajectory
sim-experiment-packet:
	@mkdir -p impl
	iverilog -g2005 -s experiment_packet_tb -o impl/experiment_packet_tb.vvp tb/experiment_packet_tb.v src/experiment_packet.v
	vvp impl/experiment_packet_tb.vvp
.PHONY: sim-experiment-packet
# Generate both vector sets with sim-domain-control/examples/compile_fixed_pd.rs.
sim-bounded-control:
	iverilog -g2005 -s fixed_pd_bounded_tb -o impl/fixed-pd/bounded.vvp impl/fixed-pd/fixed_pd_bounded_tb.v impl/fixed-pd/fixed_pd.v
	vvp impl/fixed-pd/bounded.vvp
.PHONY: sim-bounded-control

sim-experiment-reply:
	@mkdir -p impl
	iverilog -g2005 -s experiment_reply_tb -o impl/experiment_reply_tb.vvp tb/experiment_reply_tb.v src/experiment_reply.v
	vvp impl/experiment_reply_tb.vvp
check-experiment-reply:
	@mkdir -p impl
	yosys -Q -p "read_verilog src/experiment_reply.v; synth_gowin -nodsp -nolutram -top experiment_reply -json impl/experiment_reply.json; stat"
.PHONY: sim-experiment-reply check-experiment-reply

ETSRC := src/experiment_transactions.v src/experiment_scheduler.v src/experiment_packet.v src/experiment_reply.v
sim-experiment-transactions:
	@mkdir -p impl
	iverilog -g2005 -s experiment_transactions_tb -o impl/experiment_transactions_tb.vvp tb/experiment_transactions_tb.v $(ETSRC)
	vvp impl/experiment_transactions_tb.vvp
check-experiment-transactions:
	@mkdir -p impl
	yosys -Q -p "read_verilog $(ETSRC); synth_gowin -nodsp -nolutram -top experiment_transactions -json impl/experiment_transactions.json; stat"
.PHONY: sim-experiment-transactions check-experiment-transactions

sim-experiment-events:
	@mkdir -p impl
	iverilog -g2005 -s experiment_event_packet_tb -o impl/experiment_event_packet_tb.vvp tb/experiment_event_packet_tb.v src/experiment_event_packet.v
	vvp impl/experiment_event_packet_tb.vvp
check-experiment-events:
	@mkdir -p impl
	yosys -Q -p "read_verilog src/experiment_event_packet.v; synth_gowin -nodsp -nolutram -top experiment_event_packet -json impl/experiment_event_packet.json; stat"
.PHONY: sim-experiment-events check-experiment-events

sim-experiment-event-stream:
	@mkdir -p impl
	iverilog -g2005 -s experiment_event_stream_tb -o impl/experiment_event_stream_tb.vvp tb/experiment_event_stream_tb.v src/experiment_event_stream.v src/experiment_event_packet.v
	vvp impl/experiment_event_stream_tb.vvp
check-experiment-event-stream:
	@mkdir -p impl
	yosys -Q -p "read_verilog src/experiment_event_stream.v src/experiment_event_packet.v; synth_gowin -nodsp -nolutram -top experiment_event_stream -json impl/experiment_event_stream.json; stat"
.PHONY: sim-experiment-event-stream check-experiment-event-stream

sim-experiment-recording:
	@mkdir -p impl
	iverilog -g2005 -s experiment_transactions_tb -P experiment_transactions_tb.LOGGED=1 -o impl/experiment_recording_tb.vvp tb/experiment_transactions_tb.v $(ETSRC) src/experiment_event_stream.v src/experiment_event_packet.v
	vvp impl/experiment_recording_tb.vvp
.PHONY: sim-experiment-recording

# Exhaust invalid IDs after narrowing validated motor-array addresses.
sim-safety-index:
	iverilog -g2005 -s hx_safety_index_tb -o impl/hx_safety_index_tb.vvp tb/hx_safety_index_tb.v src/hx_safety.v
	vvp impl/hx_safety_index_tb.vvp
.PHONY: sim-safety-index

sim-safety-sync-scope:
	iverilog -g2005 -s hx_safety_sync_scope_tb -o impl/hx_safety_sync_scope_tb.vvp tb/hx_safety_sync_scope_tb.v src/hx_safety.v
	vvp impl/hx_safety_sync_scope_tb.vvp
.PHONY: sim-safety-sync-scope

# Alternative technology mapping; identical controller/recorder/supervisor RTL.
bridge-experiment-lut: impl/bridge_experiment_lut.fs
impl/bridge_experiment_lut.json: $(ESRC) Makefile
	yosys -p "read_verilog $(ESRC); chparam -set HOST_CLKS 50 -set RELEASE_TX 1 -set SAFETY_ENABLE 1 -set BUFFER_HOST 1 -set TRAJECTORY_ENABLE 1 -set EXPERIMENT_ENABLE 1 -set FIXED_GAINS 1 top; synth_gowin -family gw5a -strict-gw5a-dffs -nodsp -nolutram -nowidelut -noalu -top top -json $@"
impl/bridge_experiment_lut_pnr.json: impl/bridge_experiment_lut.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_experiment_lut.fs: impl/bridge_experiment_lut_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
.PHONY: bridge-experiment-lut

# 10 ms up-to-three-motor acquisition build; independent artifact paths.
# Alternative technology mapping; identical controller/recorder/supervisor RTL.
bridge-experiment-fast-loop-lut: impl/bridge_experiment_fast_loop_lut.fs
impl/bridge_experiment_fast_loop_lut.json: $(ESRC) Makefile
	yosys -p "read_verilog $(ESRC); chparam -set HOST_CLKS 50 -set RELEASE_TX 1 -set SAFETY_ENABLE 1 -set BUFFER_HOST 1 -set TRAJECTORY_ENABLE 1 -set EXPERIMENT_ENABLE 1 -set FIXED_GAINS 1 top; synth_gowin -family gw5a -strict-gw5a-dffs -nodsp -nolutram -nowidelut -noalu -top top -json $@"
impl/bridge_experiment_fast_loop_lut_pnr.json: impl/bridge_experiment_fast_loop_lut.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_experiment_fast_loop_lut.fs: impl/bridge_experiment_fast_loop_lut_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
.PHONY: bridge-experiment-fast-loop-lut

# Full UART verification of the fast, at-most-three-motor profile.
sim-bridge-experiment-10ms:
	verilator --language 1364-2005 --binary --timing -j 4 -Wno-fatal --top-module bridge_experiment_tb -GCASE=$(CASE) -GFAST=1 --Mdir impl/vexperiment_10ms_case$(CASE) tb/bridge_experiment_tb.v $(ESRC)
	./impl/vexperiment_10ms_case$(CASE)/Vbridge_experiment_tb
.PHONY: sim-bridge-experiment-10ms

# Live-reference profile retains device-owned timing and independent supervision.
bridge-live: impl/bridge_live.fs
impl/bridge_live.json: $(ESRC) Makefile
	yosys -p "read_verilog $(ESRC); chparam -set HOST_CLKS 50 -set RELEASE_TX 1 -set SAFETY_ENABLE 1 -set BUFFER_HOST 1 -set TRAJECTORY_ENABLE 1 -set EXPERIMENT_ENABLE 1 -set LIVE_ENABLE 1 -set FIXED_GAINS 2 top; synth_gowin -family gw5a -strict-gw5a-dffs -nodsp -nolutram -nowidelut -noalu -top top -json $@"
impl/bridge_live_pnr.json: impl/bridge_live.json src/bridge_safety.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=src/bridge_safety.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_live.fs: impl/bridge_live_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
.PHONY: bridge-live

sim-bridge-live:
	verilator --language 1364-2005 --binary --timing -j 4 -Wno-fatal --top-module bridge_experiment_tb -GCASE=$(CASE) -GFAST=1 -GLIVE=1 --Mdir impl/vlive_case$(CASE) tb/bridge_experiment_tb.v $(ESRC)
	./impl/vlive_case$(CASE)/Vbridge_experiment_tb
.PHONY: sim-bridge-live

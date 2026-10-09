# The supervised bridge with the ESP32 host pins: the Tang repo's own
# bridge-safety recipe (same sources, same parameters), only the constraints
# file differs. Builds impl/bridge_safety_esp32.fs here; programs nothing.
TANG   ?= $(HOME)/projects/sipeed-tang-primer-25k/servo
DEVICE ?= GW5A-LV25MG121NC1/I0
FAMILY ?= GW5A-25A
FREQ   ?= 50
SSPI   ?= sspi_as_gpio
PACKFLAGS ?= --sspi_as_gpio --done_as_gpio --ready_as_gpio
SRC := $(TANG)/src/bridge.v $(TANG)/src/uart_tx.v $(TANG)/src/uart_rx.v $(TANG)/src/hx_safety.v $(TANG)/src/button.v

impl/bridge_safety_esp32.fs: impl/bridge_safety_esp32_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
impl/bridge_safety_esp32_pnr.json: impl/bridge_safety_esp32.json bridge_safety_esp32.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=bridge_safety_esp32.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_safety_esp32.json: $(SRC)
	@mkdir -p impl
	yosys -q -p "read_verilog $(SRC); chparam -set RELEASE_TX 1 -set SAFETY_ENABLE 1 top; synth_gowin -nolutram -top top -json $@"
# The printed leg's commissioning profile with the ESP32 host pins: the Tang
# repo's bridge-calibration recipe (IDs 1-3, taught travel windows, bounded
# PWM, 400/600 ms leases), same sources and parameters, this constraints file.
impl/bridge_calibration_esp32.fs: impl/bridge_calibration_esp32_pnr.json
	gowin_pack -d $(FAMILY) -o $@ $(PACKFLAGS) $<
impl/bridge_calibration_esp32_pnr.json: impl/bridge_calibration_esp32.json bridge_safety_esp32.cst
	nextpnr-himbaechel --json $< --write $@ --device "$(DEVICE)" --vopt cst=bridge_safety_esp32.cst --vopt $(SSPI) --freq $(FREQ)
impl/bridge_calibration_esp32.json: $(SRC)
	@mkdir -p impl
	yosys -q -p "read_verilog $(SRC); chparam -set RELEASE_TX 1 -set SAFETY_ENABLE 1 -set SAFETY_FIRST_ID 1 -set SAFETY_COUNT 3 -set CALIBRATION 1 -set SAFETY_FEEDBACK_TIMEOUT 20000000 -set SAFETY_COMMAND_TIMEOUT 30000000 top; synth_gowin -nolutram -top top -json $@"
calibration: impl/bridge_calibration_esp32.fs

clean:
	rm -rf impl
.PHONY: clean calibration

# The link engine against the real supervised bridge RTL (sipeed-tang-primer-25k).
TANG ?= $(HOME)/projects/sipeed-tang-primer-25k/servo/src
RTL  := $(TANG)/bridge.v $(TANG)/hx_safety.v $(TANG)/button.v $(TANG)/uart_tx.v $(TANG)/uart_rx.v
CORE := ../core/hx.c ../core/robot_link.c ../core/gait.c
# RELEASE_TX=0: the tri-state release of servo_tx is a bus-electrical detail
# the supervisor logic does not depend on, and a plain two-state wire models it here.
PARAMS := -GSAFETY_ENABLE=1 -GRELEASE_TX=0
# The leg's calibration image (Tang repo bridge-calibration, fpga/Makefile calibration).
CAL_PARAMS := $(PARAMS) -GSAFETY_FIRST_ID=1 -GSAFETY_COUNT=3 -GCALIBRATION=1 -GSAFETY_FEEDBACK_TIMEOUT=20000000 -GSAFETY_COMMAND_TIMEOUT=30000000

# The whole robot's image (fpga/Makefile robot): IDs 1-16, re-addressing, the host link at 1 Mbaud.
ROBOT_PARAMS := $(PARAMS) -GSAFETY_FIRST_ID=1 -GSAFETY_COUNT=16 -GCALIBRATION=1 -GSAFETY_READDRESS=1 -GHOST_CLKS=50 -GSAFETY_FEEDBACK_TIMEOUT=20000000 -GSAFETY_COMMAND_TIMEOUT=30000000

run: obj_dir/Vtop obj_cal/Vtop obj_robot/Vtop
	./obj_dir/Vtop
	./obj_cal/Vtop
	./obj_robot/Vtop
robot: obj_robot/Vtop
	./obj_robot/Vtop
bench: obj_dir/Vtop
	./obj_dir/Vtop
cal: obj_cal/Vtop
	./obj_cal/Vtop
obj_dir/Vtop: sim_main.cpp world.h $(CORE) $(RTL)
	verilator --cc --exe --build -j 4 -O3 --x-assign fast --x-initial fast -Wno-fatal \
	  --top-module top $(PARAMS) -I$(TANG) $(RTL) sim_main.cpp $(CORE) \
	  -CFLAGS "-I$(CURDIR)/../core -O2" --Mdir obj_dir > build.log 2>&1 || (tail -30 build.log; false)
obj_cal/Vtop: sim_cal.cpp world.h $(CORE) $(RTL) ../test/fixtures/leg-plan.bin
	verilator --cc --exe --build -j 4 -O3 --x-assign fast --x-initial fast -Wno-fatal \
	  --top-module top $(CAL_PARAMS) -I$(TANG) $(RTL) sim_cal.cpp $(CORE) \
	  -CFLAGS "-I$(CURDIR)/../core -O2" --Mdir obj_cal > build-cal.log 2>&1 || (tail -30 build-cal.log; false)
obj_robot/Vtop: sim_robot.cpp world.h $(CORE) $(RTL) ../test/fixtures/leg-plan.bin
	verilator --cc --exe --build -j 4 -O3 --x-assign fast --x-initial fast -Wno-fatal \
	  --top-module top $(ROBOT_PARAMS) -I$(TANG) $(RTL) sim_robot.cpp $(CORE) \
	  -CFLAGS "-I$(CURDIR)/../core -O2 -DCOSIM_HOST_CLKS=50" --Mdir obj_robot > build-robot.log 2>&1 || (tail -30 build-robot.log; false)
clean:
	rm -rf obj_dir obj_cal obj_robot build.log build-cal.log build-robot.log
.PHONY: run bench cal robot clean

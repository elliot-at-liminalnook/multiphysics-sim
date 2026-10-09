CFLAGS ?= -std=c11 -Wall -Wextra -Werror -O1 -g
SRC := test_core.c fake_bridge.c ../core/hx.c ../core/robot_link.c ../core/gait.c
test: test_core
	./test_core
test_core: $(SRC) ../core/hx.h ../core/robot_link.h ../core/gait.h fake_bridge.h fixtures/leg-plan.bin
	$(CC) $(CFLAGS) -o $@ $(SRC) -lm
clean:
	rm -f test_core
.PHONY: test clean

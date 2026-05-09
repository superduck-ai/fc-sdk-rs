.RECIPEPREFIX := >

CARGO ?= cargo
FIRECRACKER_BIN ?= /data/firecracker
FC_KERNEL_DIR ?= /data_jfs/fc-kernels

.PHONY: all fmt unit-tests test all-tests doc-test examples bench-no-run check assets clean

all: check

fmt:
> $(CARGO) fmt --all

unit-tests:
> $(CARGO) test --quiet

test: all-tests

all-tests:
> $(CARGO) test --quiet

doc-test:
> $(CARGO) test --quiet --doc

examples:
> $(CARGO) check --examples

bench-no-run:
> $(CARGO) bench --no-run

check: fmt all-tests doc-test examples bench-no-run

assets:
> test -x "$(FIRECRACKER_BIN)" || (echo "missing Firecracker binary: $(FIRECRACKER_BIN)" && exit 1)
> test -d "$(FC_KERNEL_DIR)" || (echo "missing kernel directory: $(FC_KERNEL_DIR)" && exit 1)
> echo "asset check passed"

clean:
> $(CARGO) clean

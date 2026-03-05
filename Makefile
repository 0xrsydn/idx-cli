APP_NAME := rubick
BIN_DIR := bin
APP_BIN := $(BIN_DIR)/$(APP_NAME)

.PHONY: help setup build run test test-go test-py test-live e2e release clean

help:
	@echo "Targets:"
	@echo "  setup      Install Go/Python dependencies"
	@echo "  build      Build CLI binary into ./bin"
	@echo "  run        Run compiled binary: make run ARGS='msn lookup BBCA'"
	@echo "  test       Run Go and Python tests"
	@echo "  test-go    Run Go tests"
	@echo "  test-py    Run Python tests"
	@echo "  test-live  Run live network tests (requires .env and RUN_LIVE_E2E=1)"
	@echo "  e2e        Timestamped deterministic e2e run under output/<timestamp>"
	@echo "  release    Build distributable bundle under dist/ (VERSION=vX.Y.Z make release)"
	@echo "  clean      Remove local build artifacts"

setup:
	go mod download
	uv sync

build:
	mkdir -p $(BIN_DIR)
	go build -o $(APP_BIN) ./cmd/rubick

run: build
	./$(APP_BIN) $(ARGS)

test: test-go test-py

test-go:
	go test -v ./...

test-py:
	uv run python -m unittest discover -s tests -p 'test_*.py'

test-live:
	set -a; source .env; set +a; RUN_LIVE_E2E=1 go test -v ./tests/go -run TestLive

e2e: build
	set -a; source .env; set +a; bash scripts/e2e_run.sh ./$(APP_BIN)

release:
	bash scripts/release_bundle.sh $(VERSION)

clean:
	rm -rf $(BIN_DIR)

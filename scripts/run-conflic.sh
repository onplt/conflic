#!/usr/bin/env bash
set -uo pipefail

# Run conflic in GitHub Actions context
# Reads inputs from INPUT_* environment variables (set by action.yml)

SEVERITY="${INPUT_SEVERITY:-error}"
FAIL_ON="${INPUT_FAIL_ON:-error}"
SCAN_PATH="${INPUT_PATH:-.}"
DIFF_REF="${INPUT_DIFF:-}"
BASELINE="${INPUT_BASELINE:-}"
CONFIG="${INPUT_CONFIG:-}"
SARIF_UPLOAD="${INPUT_SARIF_UPLOAD:-true}"
EXTRA_ARGS="${INPUT_ARGS:-}"

# --- Build CLI arguments ---
CLI_ARGS=()
CLI_ARGS+=("$SCAN_PATH")
CLI_ARGS+=("--severity" "$SEVERITY")
CLI_ARGS+=("--no-color")

if [ -n "$DIFF_REF" ]; then
  if [ "$DIFF_REF" = "auto" ]; then
    # Resolve PR base SHA from GitHub context
    if [ -n "${GITHUB_BASE_REF:-}" ]; then
      DIFF_REF="origin/${GITHUB_BASE_REF}"
    elif [ -n "${PR_BASE_SHA:-}" ]; then
      DIFF_REF="$PR_BASE_SHA"
    else
      echo "::warning::diff=auto but no PR context found (GITHUB_BASE_REF not set). Running full scan."
      DIFF_REF=""
    fi
  fi
  if [ -n "$DIFF_REF" ]; then
    CLI_ARGS+=("--diff" "$DIFF_REF")
  fi
fi

if [ -n "$BASELINE" ]; then
  CLI_ARGS+=("--baseline" "$BASELINE")
fi

if [ -n "$CONFIG" ]; then
  CLI_ARGS+=("--config" "$CONFIG")
fi

# Append extra user-provided args
if [ -n "$EXTRA_ARGS" ]; then
  # shellcheck disable=SC2206
  CLI_ARGS+=($EXTRA_ARGS)
fi

# --- Run conflic (JSON for counts) ---
echo "::group::conflic scan"
echo "Running: conflic ${CLI_ARGS[*]} --format json"

JSON_OUTPUT=$(conflic "${CLI_ARGS[@]}" --format json 2>&1) && CONFLIC_EXIT=$? || CONFLIC_EXIT=$?

# Display terminal-friendly output
conflic "${CLI_ARGS[@]}" --format terminal 2>&1 || true

echo "::endgroup::"

# --- Parse counts from JSON ---
ERROR_COUNT=0
WARNING_COUNT=0

if [ -n "$JSON_OUTPUT" ]; then
  # Extract counts using lightweight parsing (no jq dependency)
  ERROR_COUNT=$(echo "$JSON_OUTPUT" | grep -o '"error_count":[0-9]*' | head -1 | grep -o '[0-9]*$' || echo "0")
  WARNING_COUNT=$(echo "$JSON_OUTPUT" | grep -o '"warning_count":[0-9]*' | head -1 | grep -o '[0-9]*$' || echo "0")
fi

# Fallback: derive from exit code if parsing failed
if [ "$ERROR_COUNT" = "0" ] && [ "$WARNING_COUNT" = "0" ]; then
  case $CONFLIC_EXIT in
    1) ERROR_COUNT=1 ;;
    2) WARNING_COUNT=1 ;;
  esac
fi

# --- Generate SARIF (always, for outputs; upload is controlled by action.yml) ---
SARIF_FILE="${RUNNER_TEMP:-/tmp}/conflic-results.sarif"
echo "::group::Generating SARIF"
conflic "${CLI_ARGS[@]}" --format sarif -q > "$SARIF_FILE" 2>/dev/null || true
echo "SARIF written to ${SARIF_FILE}"
echo "::endgroup::"

# --- Set outputs ---
{
  echo "exit-code=${CONFLIC_EXIT}"
  echo "error-count=${ERROR_COUNT}"
  echo "warning-count=${WARNING_COUNT}"
  echo "sarif-file=${SARIF_FILE}"
} >> "$GITHUB_OUTPUT"

# --- Apply fail-on gate ---
FINAL_EXIT=0
case "$FAIL_ON" in
  error)
    # Fail only on errors (exit code 1)
    if [ "$CONFLIC_EXIT" -eq 1 ]; then
      FINAL_EXIT=1
    fi
    ;;
  warning)
    # Fail on errors or warnings (exit code 1 or 2)
    if [ "$CONFLIC_EXIT" -eq 1 ] || [ "$CONFLIC_EXIT" -eq 2 ]; then
      FINAL_EXIT=1
    fi
    ;;
  info)
    # Fail on any findings
    if [ "$CONFLIC_EXIT" -ne 0 ]; then
      FINAL_EXIT=1
    fi
    ;;
  none)
    # Never fail
    FINAL_EXIT=0
    ;;
  *)
    echo "::error::Invalid fail-on value: ${FAIL_ON}. Must be error, warning, info, or none."
    exit 1
    ;;
esac

if [ "$FINAL_EXIT" -ne 0 ]; then
  echo "::error::conflic found issues that exceed the fail-on threshold (${FAIL_ON})"
fi

exit "$FINAL_EXIT"

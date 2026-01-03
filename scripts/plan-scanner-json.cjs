#!/usr/bin/env node
/**
 * Plan Scanner JSON Output Wrapper
 * Outputs plan metadata as JSON to stdout for Rust backend consumption
 *
 * Usage: node scripts/plan-scanner-json.cjs [plansDir]
 *
 * @module plan-scanner-json
 */

const path = require('path');
const { scanPlans } = require('../.claude/skills/plans-kanban/scripts/lib/plan-scanner.cjs');

const plansDir = process.argv[2] || './plans';
const absolutePlansDir = path.resolve(process.cwd(), plansDir);

try {
  const plans = scanPlans(absolutePlansDir);
  console.log(JSON.stringify(plans, null, 0));
} catch (err) {
  console.error(JSON.stringify({ error: err.message }));
  process.exit(1);
}

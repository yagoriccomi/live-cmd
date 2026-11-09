'use strict';

const { run, selfTest } = require('./hidden-processes.cjs');

if (process.argv.length === 3 && process.argv[2] === '--self-test') {
  selfTest().catch(error => {
    console.error(error);
    process.exitCode = 1;
  });
} else {
  run(process.argv.slice(2));
}

"""Profile a committed Spam snapshot without modifying its production scan path.

Clock instrumentation is generated only in a temporary checkout. Stage times are
estimates; the report includes instrumented wall time to expose clock overhead.
"""

import argparse
import io
import json
import platform
import re
import subprocess
import sysconfig
import tarfile
import tempfile
from pathlib import Path


PROFILE_RS = r'''
#[derive(Clone, Debug)]
pub struct ScanProfile {
    pub nanos: [u128; 4],
    pub successful_attempts: usize,
    pub failed_attempts: usize,
    pub proven_bytes: usize,
    pub prefix_lines: usize,
    pub detailed_tokens: usize,
    pub annotation_tokens: usize,
    pub annotation_frames: usize,
    pub annotation_bytes: usize,
    pub dispatches: usize,
    pub proof_samples: [usize; 2],
    pub sample_every: usize,
    pub clock_ns: u128,
    sample_seed: usize,
    proof_index: usize,
    sampled_proof: bool,
    active: usize,
    last: Option<std::time::Instant>,
}

impl Default for ScanProfile {
    fn default() -> Self {
        Self {
            nanos: [0; 4], successful_attempts: 0, failed_attempts: 0,
            proven_bytes: 0, prefix_lines: 0, detailed_tokens: 0,
            annotation_tokens: 0, annotation_frames: 0, annotation_bytes: 0,
            dispatches: 0, proof_samples: [0; 2], sample_every: 16, clock_ns: 0,
            sample_seed: 0, proof_index: 0, sampled_proof: false, active: 4, last: None,
        }
    }
}

impl ScanProfile {
    pub fn calibrated() -> Self {
        let mut empty = Self::default();
        empty.sample_every = 1;
        empty.switch(0);
        for _ in 0..1000 { empty.begin_proof(); empty.end_proof(true, 0); }
        let mut profile = Self::default();
        profile.clock_ns = empty.nanos[1] / 1000;
        profile
    }

    fn switch(&mut self, stage: usize) {
        if self.active == stage { return; }
        let now = std::time::Instant::now();
        if self.active < 4 {
            if let Some(last) = self.last {
                self.nanos[self.active] += now.duration_since(last).as_nanos();
            }
        }
        self.last = Some(now);
        self.active = stage;
    }

    fn begin_proof(&mut self) {
        self.proof_index += 1;
        self.sampled_proof = (self.proof_index + self.sample_seed) % self.sample_every == 0;
        if self.sampled_proof { self.switch(4); }
    }

    fn end_proof(&mut self, success: bool, bytes: usize) {
        if self.sampled_proof {
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(self.last.unwrap()).as_nanos().saturating_sub(self.clock_ns);
            self.nanos[if success { 1 } else { 2 }] += elapsed;
            self.proof_samples[if success { 0 } else { 1 }] += 1;
            self.last = Some(now);
            self.active = 0;
        }
        if success {
            self.successful_attempts += 1;
            self.proven_bytes += bytes;
        } else {
            self.failed_attempts += 1;
        }
    }
}

impl Checker {
    pub fn profile_regions(&mut self, source: &str, profile: &mut ScanProfile) -> Result<(), Rejection> {
        let (sample_every, clock_ns, seed) = (profile.sample_every, profile.clock_ns, profile.sample_seed + 1);
        *profile = ScanProfile::default();
        profile.sample_every = sample_every;
        profile.clock_ns = clock_ns;
        profile.sample_seed = seed;
        if source.len() > self.limits.max_source_bytes {
            return self.check(source);
        }
        let result = scan::<true, true, true>(source, &self.limits, &mut self.scratch, &mut self.work, profile);
        profile.switch(4);
        result
    }
}
'''

EXAMPLE_RS = r'''
#[path = "../tools/bench_util.rs"]
mod bench_util;
use spam::{Checker, Limits, ScanProfile};
use std::{hint::black_box, io::{self, Read}};

fn main() {
    let mut source = String::new();
    io::stdin().read_to_string(&mut source).unwrap();
    let mut checker = CONSTRUCTOR;
    let expected = checker.check(&source);
    if std::env::args().any(|arg| arg == "--check") {
        assert_eq!(checker.check_regions(&source), expected);
        match expected {
            Ok(()) => println!("accept"),
            Err(error) => println!("reject:{:?}:{}:{}", error.reason, error.range.start, error.range.end),
        }
        return;
    }

    assert_eq!(checker.check_regions(&source), expected);
    let normal = bench_util::median_ns(|| { black_box(checker.check(black_box(&source))).ok(); });
    let regions = bench_util::median_ns(|| { black_box(checker.check_regions(black_box(&source))).ok(); });
    // Calibrate the same begin/end bookkeeping on empty proofs. This does not
    // run inside measured library calls and allocates no timing sample buffer.
    let mut profile = ScanProfile::calibrated();
    let profiled = bench_util::median_ns(|| {
        black_box(checker.profile_regions(black_box(&source), &mut profile)).ok();
    });
    let mut nanos = [0u128; 4];
    let mut samples = [0usize; 2];
    for _ in 0..101 {
        assert_eq!(checker.profile_regions(&source, &mut profile), expected);
        for (sum, value) in nanos.iter_mut().zip(profile.nanos) { *sum += value; }
        for (sum, value) in samples.iter_mut().zip(profile.proof_samples) { *sum += value; }
    }
    println!("timing,{normal:.1},{regions:.1},{profiled:.1}");
    let success = if samples[0] == 0 { 0.0 } else { nanos[1] as f64 / samples[0] as f64 * profile.successful_attempts as f64 };
    let failed = if samples[1] == 0 { 0.0 } else { nanos[2] as f64 / samples[1] as f64 * profile.failed_attempts as f64 };
    let root_ns = (nanos[0] + nanos[1] + nanos[2]) as f64 / 101.0;
    println!("stages,{:.1},{success:.1},{failed:.1},{:.1}", (root_ns - success - failed).max(0.0), nanos[3] as f64 / 101.0);
    println!("sampling,{},{},{},{}", profile.sample_every, profile.clock_ns, samples[0], samples[1]);
    println!("counts,{},{},{},{},{},{},{},{},{}", profile.successful_attempts, profile.failed_attempts,
        profile.proven_bytes, profile.prefix_lines, profile.detailed_tokens, profile.annotation_tokens,
        profile.annotation_frames, profile.annotation_bytes, profile.dispatches);
}
'''


def replace_once(source, old, new):
    if source.count(old) != 1:
        raise ValueError(f'Unsupported scanner layout: expected one {old[:70]!r}')
    return source.replace(old, new, 1)


def keep_live_lexer(root):
    """Historical experiment: suspend the lexer only when entering an annotation."""
    path = root / 'src/lib.rs'
    source = path.read_text()
    source = replace_once(source, '        #[cfg(feature = "experimental-regions")]\n        if REGIONS',
                          '        let next_child = loop {\n        #[cfg(feature = "experimental-regions")]\n        if REGIONS')
    source = replace_once(source, '            current.lexer = Some(lexer.state());\n            continue;',
                          '            continue;')
    source = replace_once(source, '        let Some(token) = lexer.next() else {\n            work.pop();\n            continue;\n        };\n        current.lexer = Some(lexer.state());',
                          '        let Some(token) = lexer.next() else { break None; };')
    source = replace_once(source, '        if let Some(child) = child {\n            work.push(child);\n        }',
                          '        if let Some(child) = child {\n            current.lexer = Some(lexer.state());\n            break Some(child);\n        }\n        };\n        if let Some(child) = next_child {\n            work.push(child);\n        } else {\n            work.pop();\n        }')
    path.write_text(source)


def instrument(root):
    path = root / 'src/lib.rs'
    source = path.read_text()
    fallible = bool(re.search(r'pub fn new\(limits: Limits\) -> Result', source))
    source = source.replace('scan::<true, REGIONS>(', 'scan::<true, REGIONS, false>(')
    source = replace_once(source, 'fn scan<const FAST: bool, const REGIONS: bool>(',
                          'fn scan<const FAST: bool, const REGIONS: bool, const PROFILE: bool>(')
    source = replace_once(source, '    work: &mut Vec<ScanWork>,\n) -> Result<(), Rejection>',
                          '    work: &mut Vec<ScanWork>,\n    profile: &mut ScanProfile,\n) -> Result<(), Rejection>')
    source = replace_once(source, 'scan::<true, REGIONS, false>(source, &self.limits, &mut self.scratch, &mut self.work)',
                          'scan::<true, REGIONS, false>(source, &self.limits, &mut self.scratch, &mut self.work, &mut ScanProfile::default())')
    source = replace_once(source, '        let current = &mut work[index];', '''        if PROFILE {
            profile.switch(if index == 0 { 0 } else { 3 });
            profile.dispatches += 1;
        }
        let current = &mut work[index];''')
    # Group the whole simple-prefix scan under one pair of clock reads. Per-line
    # clocks would overwhelm these very small proofs and distort their cost.
    source = replace_once(source, '            if FAST {', '''            if PROFILE && index == 0 { profile.begin_proof(); }
            let prefix_before = lexer.profile_position();
            if FAST {''')
    prefix_counter = 'tokens_used' if 'tokens_used' in source else 'current.token_count'
    source = replace_once(source, f'                    {prefix_counter} += count;',
                          f'                    if PROFILE && index == 0 {{ profile.prefix_lines += 1; }}\n                    {prefix_counter} += count;')
    source = replace_once(source, '            lexer\n        };', '''            if PROFILE && index == 0 {
                let bytes = lexer.profile_position() - prefix_before;
                profile.end_proof(bytes > 0, bytes);
            }
            lexer
        };''')
    pattern = re.compile(r'(?P<prefix>(?P<indent> +)#\[cfg\(feature = "experimental-regions"\)\]\n(?P=indent)if REGIONS.*?&& let Some\(remaining\) = scanner.region_budget\(\))\n +&& let Some\(\(count, seen_code\)\) = lexer.skip_region\((?P<args>.*?)\n +\)\n +\{(?P<body>.*?)\n(?P=indent)\}', re.S)
    match = pattern.search(source)
    if not match:
        raise ValueError('Unsupported region-proof dispatch layout')
    indent = match['indent']
    replacement = (match['prefix'] + ' {\n'
                   + indent + '    let before = lexer.profile_position();\n'
                   + indent + '    if PROFILE && index == 0 { profile.begin_proof(); }\n'
                   + indent + '    let proof = lexer.skip_region(' + match['args'] + ');\n'
                   + indent + '    if PROFILE && index == 0 {\n'
                   + indent + '        profile.end_proof(proof.is_some(), lexer.profile_position() - before);\n'
                   + indent + '    }\n'
                   + indent + '    if let Some((count, seen_code)) = proof {' + match['body'] + '\n'
                   + indent + '    }\n' + indent + '}')
    source = source[:match.start()] + replacement + source[match.end():]
    source, count = re.subn(r'(?m)^([ \t]*)current.region_start =', r'\1if PROFILE {\n\1    if index == 0 { profile.detailed_tokens += 1; } else { profile.annotation_tokens += 1; }\n\1}\n\1current.region_start =', source)
    if count != 1:
        raise ValueError('Unsupported token-dispatch layout')
    source = replace_once(source, '            work.push(child);', '''            if PROFILE {
                profile.annotation_frames += 1;
                profile.annotation_bytes += child.source.len();
            }
            work.push(child);''')
    path.write_text(source + PROFILE_RS)
    lexer = root / 'src/lexer.rs'
    text = lexer.read_text()
    lexer.write_text(replace_once(text, '    pub(super) fn state(&self) -> LexerState {',
                                  '    pub(super) fn profile_position(&self) -> usize { self.position }\n\n    pub(super) fn state(&self) -> LexerState {'))
    example = root / 'examples/region_profile.rs'
    example.parent.mkdir(exist_ok=True)
    constructor = 'Checker::new(Limits::default())' + ('.expect("valid benchmark limits")' if fallible else '')
    example.write_text(EXAMPLE_RS.replace('CONSTRUCTOR', constructor))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--revision', default='HEAD')
    parser.add_argument('--live-lexer', action='store_true', help='Test retaining lexer state between annotation suspensions')
    parser.add_argument('--keep-build', type=Path, help='Copy the instrumented benchmark executable here for result comparisons')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--stdlib-file', action='append', default=[])
    args = parser.parse_args()
    revision = subprocess.check_output(['git', 'rev-parse', args.revision], text=True).strip()
    stdlib = Path(sysconfig.get_path('stdlib'))
    sources = {
        'tiny': 'x = 1 + 2\n',
        'flat-60k': 'x = 1\n' * 10_000,
        'chain-80': 'x = ' + '1 + ' * 80 + '1\n',
        'annotations': ('x: "list[\'' + 'word ' * 30 + '\']"\n') * 100,
    }
    names = args.stdlib_file or ['shutil.py', 'ast.py', 'dataclasses.py', 'functools.py', 'json/decoder.py', 're/__init__.py']
    sources.update({name: (stdlib / name).read_text() for name in names})
    with tempfile.TemporaryDirectory(prefix='spam-profile-') as directory:
        root = Path(directory)
        archive = subprocess.check_output(['git', 'archive', revision, 'Cargo.toml', 'Cargo.lock', 'src', 'tools/bench_util.rs'])
        with tarfile.open(fileobj=io.BytesIO(archive)) as contents:
            contents.extractall(root, filter='data')
        if args.live_lexer:
            keep_live_lexer(root)
        instrument(root)
        subprocess.run(['cargo', 'build', '--offline', '--release', '--features', 'experimental-regions',
                        '--example', 'region_profile'], cwd=root, check=True)
        if args.keep_build:
            import shutil
            shutil.copy2(root / 'target/release/examples/region_profile', args.keep_build)
        results = []
        for name, source in sources.items():
            run = subprocess.run([str(root / 'target/release/examples/region_profile')], input=source.encode(),
                                 capture_output=True, check=True)
            rows = {parts[0]: parts[1:] for line in run.stdout.decode().splitlines() if (parts := line.split(','))}
            normal, regions, profiled = map(float, rows['timing'])
            stages = dict(zip(['detailed_and_dispatch', 'successful_proofs', 'failed_proofs', 'annotations'], map(float, rows['stages'])))
            counts = dict(zip(['successful_attempts', 'failed_attempts', 'proven_bytes', 'prefix_lines', 'detailed_tokens',
                               'annotation_tokens', 'annotation_frames', 'annotation_bytes', 'work_dispatches'], map(int, rows['counts'])))
            result = {'name': name, 'bytes': len(source.encode()), 'normal_ns': normal, 'regions_ns': regions,
                      'profiled_ns': profiled, 'stage_ns': stages, 'counts': counts,
                      'sampling': dict(zip(['every', 'clock_ns', 'successful_samples', 'failed_samples'], map(int, rows['sampling'])))}
            results.append(result)
            total = sum(stages.values())
            shares = ', '.join(f'{stage} {ns / total if total else 0:.0%}' for stage, ns in stages.items())
            print(f'{name}: regions {regions / 1000:.2f} µs, instrumented {profiled / regions:.2f}×; {shares}', flush=True)
        args.output.write_text(json.dumps({'revision': revision, 'live_lexer': args.live_lexer, 'platform': platform.platform(), 'results': results}, indent=2) + '\n')


if __name__ == '__main__':
    main()

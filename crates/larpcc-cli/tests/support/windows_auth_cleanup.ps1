param([ValidateSet('before','after')][string]$Phase = 'after')
$ErrorActionPreference = 'Stop'
# Cargo's expected negative result is validated below, not raised by pwsh.
$PSNativeCommandUseErrorActionPreference = $false
if (-not $IsWindows -or -not $env:LARPCC_TEST_PARENT) { throw 'Native Windows with verified private project-disk fixtures required.' }
$env:CARGO_BUILD_JOBS = '2'
$head = & git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify native source HEAD.' }
$tree = & git rev-parse 'HEAD^{tree}'
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify native source tree.' }
Write-Output "Auth cleanup phase=$Phase HEAD=$head tree=$tree"
if ($Phase -eq 'before') {
    if ($head -ne 'c2f42db51929f48d7b7a925d763b3189640acb51' -or $tree -ne '016e7a9afd904225f2dc07e42e4415303ff5cfe2') { throw 'Negative control requires exact vulnerable J2.' }
    $patchSource = Join-Path $PSScriptRoot '../fixtures/windows/auth-cleanup-before.patch'
    $patch = Join-Path $env:LARPCC_TEST_PARENT 'auth-cleanup-before.patch'
    # Undo only checkout CRLF transport; verify original fixture bytes before use.
    [IO.File]::WriteAllText($patch, [IO.File]::ReadAllText($patchSource).Replace("`r`n", "`n"), [Text.UTF8Encoding]::new($false))
    if ((Get-FileHash -Algorithm SHA256 $patch).Hash.ToLowerInvariant() -ne '53b8932505386490318cee8226b409b05dd2a0a870df3651951c35b3dfe93489') { throw 'Negative fixture digest differs.' }
    & git apply --check --index $patch
    if ($LASTEXITCODE -ne 0) { throw 'Negative fixture check failed.' }
    & git apply --index $patch
    if ($LASTEXITCODE -ne 0) { throw 'Negative fixture application failed.' }
    $tree = & git write-tree
    if ($LASTEXITCODE -ne 0 -or $tree -ne '1bde3f0a623dc15ccad6e75dff6c9853f1ac2605') { throw 'Negative control test-only tree differs.' }
    Write-Output "Auth cleanup phase=$Phase HEAD=$head patched-tree=$tree"
}
$prefix = 'launch::windows::lifecycle::'
$names = @('stage3_auth_wait_failure_retains_guard_past_cleanup_deadline_until_reap')
if ($Phase -eq 'after') {
    $names += @('stage3_run_wait_failure_keeps_ordinary_bounded_cleanup', 'w04_failure_paths_never_report_success', 'w04_ctrl_c_and_break_deliver_exactly_once_without_forwarding', 'w05_no_window_preserves_full_u32_exit_status')
}
foreach ($short in $names) {
    $name = $prefix + $short
    Write-Output "Auth cleanup phase=$Phase exact-test=$name"
    $listed = & cargo +stable test --locked --offline -p larpcc-cli --bin larp $name -- --exact --list 2>&1
    $listed | Write-Output
    if ($LASTEXITCODE -ne 0 -or @($listed | Where-Object { "$_" -match ('^' + [regex]::Escape($name) + ': test$') }).Count -ne 1) { throw 'Exactly one listed native test required.' }
    $result = & cargo +stable test --locked --offline -p larpcc-cli --bin larp $name -- --exact --nocapture 2>&1
    $code = $LASTEXITCODE
    $result | Write-Output
    Write-Output "Auth cleanup phase=$Phase exact-test=$name cargo-exit=$code"
    if ($Phase -eq 'before') {
        if ($code -eq 0 -or -not ($result | Where-Object { "$_" -match '^test result: FAILED\. 0 passed; 1 failed; 0 ignored;' }) -or -not ($result | Where-Object { "$_" -match 'dispatch returned before confirmed reap|wrapper returned or panicked before release' })) { throw 'Negative control must fail for the guard/reap assertion, not a build failure or empty filter.' }
    } elseif ($code -ne 0 -or -not ($result | Where-Object { "$_" -match '^test result: ok\. 1 passed; 0 failed; 0 ignored;' })) { throw 'One executed native pass required.' }
}
# The GitHub pwsh wrapper propagates LASTEXITCODE unless the validated failure is reset.
$global:LASTEXITCODE = 0

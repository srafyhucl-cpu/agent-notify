$ErrorActionPreference = 'Stop'

$llvmHome = if ($env:AGENTNOTIFY_LLVM_HOME) {
    $env:AGENTNOTIFY_LLVM_HOME
} else {
    'D:\Tools\LLVM-23.1.1\LLVM'
}
$cargoXwinHome = if ($env:AGENTNOTIFY_CARGO_XWIN_HOME) {
    $env:AGENTNOTIFY_CARGO_XWIN_HOME
} else {
    'D:\Tools\cargo-xwin'
}
$xwinCacheDir = if ($env:AGENTNOTIFY_XWIN_CACHE_DIR) {
    $env:AGENTNOTIFY_XWIN_CACHE_DIR
} else {
    'D:\Tools\xwin-cache'
}

$llvmBin = Join-Path $llvmHome 'bin'
$requiredFiles = @(
    (Join-Path $cargoXwinHome 'cargo-xwin.exe'),
    (Join-Path $llvmBin 'clang-cl.exe'),
    (Join-Path $llvmBin 'lld-link.exe'),
    (Join-Path $llvmBin 'llvm-lib.exe')
)
foreach ($file in $requiredFiles) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        throw "xwin fallback file not found: $file"
    }
}

New-Item -ItemType Directory -Force -Path $xwinCacheDir | Out-Null

$target = 'x86_64-pc-windows-msvc'
$cacheRoot = $xwinCacheDir.Replace('\', '/')
$includePaths = @(
    "$cacheRoot/xwin/crt/include",
    "$cacheRoot/xwin/sdk/include/ucrt",
    "$cacheRoot/xwin/sdk/include/um",
    "$cacheRoot/xwin/sdk/include/shared",
    "$cacheRoot/xwin/sdk/include/winrt"
)
$libraryPaths = @(
    "$cacheRoot/xwin/crt/lib/x86_64",
    "$cacheRoot/xwin/sdk/lib/um/x86_64",
    "$cacheRoot/xwin/sdk/lib/ucrt/x86_64"
)
$includeArgs = ($includePaths | ForEach-Object { "/imsvc $_" }) -join ' '
$rustFlags = @(
    '-C linker-flavor=lld-link',
    "-Lnative=$cacheRoot/xwin/crt/lib/x86_64",
    "-Lnative=$cacheRoot/xwin/sdk/lib/um/x86_64",
    "-Lnative=$cacheRoot/xwin/sdk/lib/ucrt/x86_64"
) -join ' '
$nativeFlags = "--target=$target -Wno-unused-command-line-argument -fuse-ld=lld-link $includeArgs"

$env:PATH = "$cargoXwinHome;$llvmBin;$env:PATH"
$env:XWIN_CACHE_DIR = $xwinCacheDir
$env:XWIN_ARCH = 'x86_64'
$env:XWIN_CROSS_COMPILER = 'clang-cl'
$env:BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_msvc = ($includePaths | ForEach-Object { "-I$_" }) -join ' '
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'lld-link'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $rustFlags
$env:CC_x86_64_pc_windows_msvc = 'clang-cl'
$env:CFLAGS_x86_64_pc_windows_msvc = $nativeFlags
$env:CL_FLAGS = $nativeFlags
$env:CXX_x86_64_pc_windows_msvc = 'clang-cl'
$env:CXXFLAGS_x86_64_pc_windows_msvc = $nativeFlags
$env:AR_x86_64_pc_windows_msvc = 'llvm-lib'
$env:LIB = $libraryPaths -join ';'
$env:RCFLAGS = ($includePaths | ForEach-Object { "-I$_" }) -join ' '
$env:TARGET_AR = 'llvm-lib'
$env:TARGET_CC = 'clang-cl'
$env:TARGET_CXX = 'clang-cl'
Remove-Item 'Env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER' -ErrorAction SilentlyContinue

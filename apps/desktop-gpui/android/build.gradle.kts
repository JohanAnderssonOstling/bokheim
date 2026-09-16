plugins {
    id("com.android.application") version "8.11.0"
}

val androidNdkVersion = "29.0.13599879"
val rustTarget = "aarch64-linux-android"
val rustAbi = "arm64-v8a"
val workspaceRoot = rootProject.projectDir.resolve("../../..").canonicalFile
val rustDebugLibrary = workspaceRoot.resolve("target/$rustTarget/debug/libdesktop_gpui.so")
val rustReleaseLibrary = workspaceRoot.resolve("target/$rustTarget/release/libdesktop_gpui.so")
val stagedRustDebugLibraries = layout.buildDirectory.dir("rustJniLibs/debug")
val stagedRustReleaseLibraries = layout.buildDirectory.dir("rustJniLibs/release")

android {
    namespace = "se.bokheim.reader.gpui"
    compileSdk = 36
    ndkVersion = androidNdkVersion

    defaultConfig {
        applicationId = "se.bokheim.reader.gpui"
        ndk { abiFilters += rustAbi }
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.6"
    }

    testOptions.unitTests.isIncludeAndroidResources = true

    sourceSets.getByName("main").manifest.srcFile("AndroidManifest.xml")
    sourceSets.getByName("debug").jniLibs.srcDir(stagedRustDebugLibraries)
    sourceSets.getByName("release").jniLibs.srcDir(stagedRustReleaseLibraries)

    buildTypes {
        getByName("release") {
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.16.1")
    implementation("androidx.media3:media3-exoplayer:1.8.0")
    implementation("androidx.media3:media3-session:1.8.0")
    implementation(platform("org.jetbrains.kotlin:kotlin-bom:1.8.22"))
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.core:core:1.16.0")
}

val preparePdfium by tasks.registering(Exec::class) {
    workingDir(workspaceRoot)
    commandLine("python3", "shared/pdfium/prepare.py", "--target", rustTarget)
}

val buildRustDebug by tasks.registering(Exec::class) {
    dependsOn(preparePdfium)
    workingDir(workspaceRoot)
    commandLine(
        "cargo",
        "build",
        "-p",
        "desktop-gpui",
        "--lib",
        "--no-default-features",
        "--features",
        "android",
        "--target",
        rustTarget,
    )
    doFirst {
        val configuredNdk = providers.gradleProperty("rust.ndkHome").orNull
            ?: System.getenv("ANDROID_NDK_HOME")
            ?: System.getenv("NDK_HOME")
            ?: System.getenv("ANDROID_HOME")?.let { "$it/ndk/$androidNdkVersion" }
            ?: throw GradleException("Set rust.ndkHome, ANDROID_NDK_HOME, NDK_HOME, or ANDROID_HOME")
        val toolchain = file(configuredNdk).resolve("toolchains/llvm/prebuilt/linux-x86_64/bin")
        val clang = toolchain.resolve("aarch64-linux-android26-clang")
        val clangCpp = toolchain.resolve("aarch64-linux-android26-clang++")
        val archiveTool = toolchain.resolve("llvm-ar")
        if (!clang.isFile || !clangCpp.isFile || !archiveTool.isFile) {
            throw GradleException("Android NDK toolchain is incomplete under ${file(configuredNdk)}")
        }
        environment("CC_aarch64_linux_android", clang.absolutePath)
        environment("CXX_aarch64_linux_android", clangCpp.absolutePath)
        environment("AR_aarch64_linux_android", archiveTool.absolutePath)
        environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", clang.absolutePath)
    }
}

val stageRustDebug by tasks.registering(Copy::class) {
    dependsOn(buildRustDebug)
    from(rustDebugLibrary)
    from(workspaceRoot.resolve(".pdfium/$rustTarget/libpdfium.so"))
    into(stagedRustDebugLibraries.map { it.dir(rustAbi) })
}

val buildRustRelease by tasks.registering(Exec::class) {
    dependsOn(preparePdfium)
    workingDir(workspaceRoot)
    commandLine(
        "cargo",
        "build",
        "-p",
        "desktop-gpui",
        "--lib",
        "--release",
        "--no-default-features",
        "--features",
        "android",
        "--target",
        rustTarget,
    )
    doFirst {
        val configuredNdk = providers.gradleProperty("rust.ndkHome").orNull
            ?: System.getenv("ANDROID_NDK_HOME")
            ?: System.getenv("NDK_HOME")
            ?: System.getenv("ANDROID_HOME")?.let { "$it/ndk/$androidNdkVersion" }
            ?: throw GradleException("Set rust.ndkHome, ANDROID_NDK_HOME, NDK_HOME, or ANDROID_HOME")
        val toolchain = file(configuredNdk).resolve("toolchains/llvm/prebuilt/linux-x86_64/bin")
        val clang = toolchain.resolve("aarch64-linux-android26-clang")
        val clangCpp = toolchain.resolve("aarch64-linux-android26-clang++")
        val archiveTool = toolchain.resolve("llvm-ar")
        if (!clang.isFile || !clangCpp.isFile || !archiveTool.isFile) {
            throw GradleException("Android NDK toolchain is incomplete under ${file(configuredNdk)}")
        }
        environment("CC_aarch64_linux_android", clang.absolutePath)
        environment("CXX_aarch64_linux_android", clangCpp.absolutePath)
        environment("AR_aarch64_linux_android", archiveTool.absolutePath)
        environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", clang.absolutePath)
    }
}

val stageRustRelease by tasks.registering(Copy::class) {
    dependsOn(buildRustRelease)
    from(rustReleaseLibrary)
    from(workspaceRoot.resolve(".pdfium/$rustTarget/libpdfium.so"))
    into(stagedRustReleaseLibraries.map { it.dir(rustAbi) })
}

tasks.configureEach {
    if (name == "mergeDebugJniLibFolders" || name == "mergeDebugNativeLibs") {
        dependsOn(stageRustDebug)
    }
    if (name == "mergeReleaseJniLibFolders" || name == "mergeReleaseNativeLibs") {
        dependsOn(stageRustRelease)
    }
}

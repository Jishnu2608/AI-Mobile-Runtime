plugins {
  alias(libs.plugins.android.application)
}

android {
  namespace = "com.tumr.samples.javacalc"
  compileSdk = 36
  defaultConfig {
    applicationId = "com.tumr.samples.javacalc"
    minSdk = 24
    targetSdk = 36
    versionCode = 1
    versionName = "1.0"
  }
  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
}

dependencies {
  // the app uses only Android's own widgets
  testImplementation(libs.junit)
}

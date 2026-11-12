plugins { alias(libs.plugins.android.application) }
android {
    namespace = "dev.umr.sample"
    compileSdk = 36
    defaultConfig {
        minSdk = 24
        targetSdk = 36
    }
}

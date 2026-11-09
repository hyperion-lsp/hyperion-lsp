namespace java com.hyperion.example.core.thrift

/** A deliberately small schema for the Gradle resolution fixture. */
struct UserProfile {
  1: required string id
  2: optional string displayName
}

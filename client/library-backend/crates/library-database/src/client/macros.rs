//! Typed async methods use the shared connection access policy.
macro_rules! database_methods {
    ($(
        $(#[$attribute:meta])*
        fn $method:ident($($parameter:ident: $parameter_type:ty),* $(,)?) -> $output:ty;
        arguments $arguments_type:ty = $arguments:expr;
        execute $database:ident, $values:ident => $execute:expr;
    )*) => {
        impl crate::DatabaseClient {
            $(
                $(#[$attribute])*
                pub async fn $method(&self, $($parameter: $parameter_type),*) -> Result<$output, crate::DatabaseError> {
                    let arguments: $arguments_type = $arguments;
                    self.run(move |$database| {
                        let $values = arguments;
                        let _ = &$values;
                        $execute
                    }).await
                }
            )*
        }
    };
}

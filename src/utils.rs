pub fn with_report_progress<T>(
    increment: u64,
    mut msg: impl FnMut(u64),
    f: impl FnOnce(&mut dyn FnMut()) -> T,
) -> T {
    let mut i = 0;
    (msg)(i);
    let ret = f(&mut || {
        i += 1;
        if i % increment == 0 {
            (msg)(i);
        }
    });
    (msg)(i);
    ret
}

//! A program in a container (`docker run …`, `podman run …`): given the
//! host's network and the recording's variables by name, so that it reaches
//! sonde on 127.0.0.1 and no key is written on a command line.

/// The options of `docker run` / `podman run` that take a value as the next
/// word (`-e NAME`, `--network host`); every other option takes none, or its
/// value after `=`.
const TAKES_VALUE: &[&str] = &[
    "-a", "--attach", "--add-host", "--cap-add", "--cap-drop", "--cidfile", "--cpus",
    "--cpuset-cpus", "-c", "--cpu-shares", "--device", "--dns", "-e", "--env", "--env-file",
    "--entrypoint", "--expose", "--gpus", "-h", "--hostname", "--ip", "-l", "--label",
    "--log-driver", "--log-opt", "-m", "--memory", "--mount", "--name", "--network", "--net",
    "--platform", "--pull", "-p", "--publish", "--restart", "--runtime", "--security-opt",
    "--shm-size", "--stop-signal", "--tmpfs", "-u", "--user", "--ulimit", "-v", "--volume",
    "--volumes-from", "-w", "--workdir", "--userns", "--pid", "--ipc", "--uts",
];

/// Where `run`'s options start in the words after `docker` or `podman`
/// (`run …`, `container run …`): None for another subcommand.
fn run_at(args: &[&str]) -> Option<usize> {
    match args {
        ["run", ..] => Some(1),
        ["container", "run", ..] => Some(2),
        _ => None,
    }
}

/// The image `docker run` / `podman run` runs, of the words after `docker`
/// or `podman`: the first that is no option nor an option's value.
pub fn image<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut it = args.iter().skip(run_at(args)?);
    while let Some(w) = it.next() {
        if *w == "--" {
            return it.next().copied();
        }
        if w.starts_with('-') {
            if TAKES_VALUE.contains(w) {
                it.next();
            }
            continue;
        }
        return Some(w);
    }
    None
}

/// `docker run …` (or podman's) with the host's network and each variable
/// passed by name (`-e AWS_ENDPOINT_URL`: its value is the docker client's,
/// which sonde sets), and each file a variable names mounted where it is (a
/// `--tls` certificate). None for any other command. A network the command
/// names itself is kept, and said by the caller ([`own_network`]).
pub fn with_settings(words: &[String], names: &[String], files: &[String]) -> Option<Vec<String>> {
    let program = words.first()?.rsplit(['/', '\\']).next()?;
    let program = program.strip_suffix(".exe").unwrap_or(program);
    if !matches!(program, "docker" | "podman") {
        return None;
    }
    let args: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    let at = 1 + run_at(&args)?;
    let mut added = Vec::new();
    if own_network(words).is_none() {
        added.extend(["--network".to_string(), "host".to_string()]);
    }
    for n in names {
        added.extend(["-e".to_string(), n.clone()]);
    }
    for f in files {
        added.extend(["-v".to_string(), format!("{f}:{f}:ro")]);
    }
    let mut out = words[..at].to_vec();
    out.extend(added);
    out.extend_from_slice(&words[at..]);
    Some(out)
}

/// The network a `docker run` names itself (`--network bridge`,
/// `--net=none`), before its image.
pub fn own_network(words: &[String]) -> Option<String> {
    let mut it = words.iter().skip(1);
    while let Some(w) = it.next() {
        if !w.starts_with('-') && !matches!(w.as_str(), "run" | "container") {
            return None;
        }
        if let Some(v) = w
            .strip_prefix("--network=")
            .or_else(|| w.strip_prefix("--net="))
        {
            return Some(v.to_string());
        }
        if matches!(w.as_str(), "--network" | "--net") {
            return it.next().cloned();
        }
        if TAKES_VALUE.contains(&w.as_str()) {
            it.next();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(c: &str) -> Vec<String> {
        c.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn a_container_is_given_the_hosts_network_and_the_variables_by_name() {
        let names = ["AWS_ENDPOINT_URL".to_string(), "AWS_ACCESS_KEY_ID".to_string()];
        let got = with_settings(
            &words("docker run --rm -v /data:/data amazon/aws-cli s3 ls"),
            &names,
            &[],
        )
        .unwrap();
        assert_eq!(
            got.join(" "),
            "docker run --network host -e AWS_ENDPOINT_URL -e AWS_ACCESS_KEY_ID --rm -v /data:/data amazon/aws-cli s3 ls"
        );
        // no value of a variable is on the command line
        assert!(!got.iter().any(|w| w.contains('=')));
        let got = with_settings(&words("podman container run img"), &names[..1], &["/c.pem".into()])
            .unwrap();
        assert_eq!(
            got.join(" "),
            "podman container run --network host -e AWS_ENDPOINT_URL -v /c.pem:/c.pem:ro img"
        );
        // a network of its own is kept
        let own = words("docker run --network bridge img");
        assert_eq!(own_network(&own).as_deref(), Some("bridge"));
        let got = with_settings(&own, &names[..1], &[]).unwrap();
        assert!(!got.contains(&"host".to_string()), "{got:?}");
        // not a run, not a container
        assert!(with_settings(&words("docker ps"), &names, &[]).is_none());
        assert!(with_settings(&words("python job.py"), &names, &[]).is_none());
    }

    #[test]
    fn the_image_is_the_first_word_no_option_takes() {
        let img = |c: &str| {
            let w: Vec<&str> = c.split_whitespace().collect();
            image(&w).map(String::from)
        };
        assert_eq!(img("run --rm -e A -v a:b amazon/aws-cli s3 ls").as_deref(), Some("amazon/aws-cli"));
        assert_eq!(img("run --name=x --network host rclone/rclone:1.76 ls").as_deref(), Some("rclone/rclone:1.76"));
        assert_eq!(img("container run -it img").as_deref(), Some("img"));
        assert_eq!(img("ps -a"), None);
    }
}

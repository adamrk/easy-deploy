# easy-deploy
Deploy files to a central location as easily as copying

If you're deploying by copying binaries or config files to a central location, this tool will keep track of old versions and allow easy rollbacks.

To deploy `my_binary.exe` to `central/location/binary`
```
$ easy-deploy my_binary.exe central/location/binary --message 'added new features'
```

To rollback:
```
$ easy-deploy rollback central/location/binary --message 'uh oh, rollback'
```

To see the history:
```
$ easy-deploy list central/location/binary
+----+---------------------+---------------------+---------+
| id | message             | timestamp           | current |
+----+---------------------+---------------------+---------+
| 2  | something was wrong | 2020-29-02 18:30:04 | *       |
+----+---------------------+---------------------+---------+
| 3  | v4                  | 2020-29-02 18:02:49 |         |
+----+---------------------+---------------------+---------+
| 2  | v3                  | 2020-27-02 13:55:12 |         |
+----+---------------------+---------------------+---------+
| 1  | v2                  | 2020-25-02 07:34:44 |         |
+----+---------------------+---------------------+---------+
| 0  | v1                  | 2020-15-02 12:00:40 |         |
+----+---------------------+---------------------+---------+
```

You can also rollback to a specific version:
```
$ easy-deploy rollback central/location/binary --version 3
```

## Note
The current setup only saves 10 versions.

## How it works
Nothing complicated - we're just creating a hidden file for each deployment and a symlink pointing to the current version.

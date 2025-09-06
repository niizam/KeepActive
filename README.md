# Keep Active
KeepActive is a CLI program written in C that helps me keep a specified window active on Windows. It's particularly useful for applications or games that require constant active window to not get muted. (e.g. CounterSide)

## Requirements

- Windows
- GCC compiler with pthreads support

## Compilation

To compile the program, use the following command:

```
gcc -o KeepActive.exe KeepActive.c -lpthread -lws2_32
```

Make sure you have GCC installed and properly set up in your system PATH.

## Usage

1. Run the compiled program:
   ```
   .\KeepActive
   ```
   This will start the program with the default window name "CounterSide".

2. To specify a custom window name, use the `-w` flag followed by the window name:
   ```
   .\KeepActive -w "Your Window Name"
   ```

3. Once the program is running, you can use the following commands:
   - Type `1` and press Enter to start keeping the window active
   - Type `0` and press Enter to stop keeping the window active
   - Type `q` and press Enter to quit the program

## Contributing

Contributions to improve the program are welcome. Please feel free to submit pull requests or open issues for bugs and feature requests.

## License

This project is open source and available under the [MIT License](https://opensource.org/licenses/MIT).

## Disclaimer

This software is provided as-is, without any guarantees or warranty. The authors are not responsible for any damage or data loss that may occur from using this program.

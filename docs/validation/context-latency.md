# Indexed terms: latency with a large repository

## Cause

A repository index holds up to 50,000 terms, and one large repository reaches that limit.
Several steps read every term on each use:

- Preparing a result ran a regex over each term, built the sound-key table for the whole vocabulary, and cloned the context snapshot, each time it computed suggestions. Enhanced cleanup computes them a second time.
- Starting a recording copied every term into the context snapshot and ranked them for the recognition hint before the microphone opened.
- Every file change in a watched repository started a scan that read and parsed every file, then wrote the whole index to the database while holding the lock that the shortcut, the interface poll, and the recording indicator wait on.
- The status query that the interface poll runs read past the stored entries of each row to reach the status columns.
- A page of the Repositories term list parsed the stored JSON on every request.

## Changes

- The terms of a context snapshot are shared by its clones with the tables derived from them. The tables are built once, on a background thread when the recording starts.
- Text that lacks a byte of a term is not searched for that term.
- Recordings that use the same indexes share one set of terms.
- A scan keeps what it read from each file and reads only files whose size or modification time changed. The Refresh button reads every file.
- An index is written without the app lock, and a scan that found the stored index writes only the time.
- Schema 2 stores the status columns before the entries.
- The term list reads the index held in memory.

## Measurements

`examples/context_latency.rs` produced these numbers on one machine: Intel Xeon E5-2630 v3, Linux, release build.
It scanned two repositories, one at the 50,000-term limit and this repository (929 terms), 50,929 terms together.
The dictation text has 103 bytes; the long text repeats it 12 times (1,236 bytes).
Each row is the range over the runs made during the work, three to five per row.
No Apple Silicon Mac was measured, and no recording was made: the numbers cover the functions named, without capture, recognition, or delivery.

| Step | Before | After |
| --- | --- | --- |
| Prepare the result, 103 bytes | 131-140 ms | 8.3-8.7 ms |
| Prepare the result, 1,236 bytes | 180-190 ms | 27-29 ms |
| Suggestions alone, 103 bytes | 129-133 ms | 4.7-4.9 ms |
| Mask alone, 103 bytes | 8.1-8.5 ms | 3.0-3.3 ms |
| Context snapshot at recording start | 9.9-16 ms | 2.5-2.6 microseconds when the indexes are the same |
| Clone of the context snapshot | 7.8 ms | 0.2 microseconds |
| Recognition hint | 21-24 ms | 0.5-0.6 ms after the first for a set of terms |
| Scan of the 50,000-term repository | 3.3-4.0 s every time | 32-34 ms when no file changed |
| Index status query | 1.9-2.3 ms | 0.13-0.15 ms |
| Page of the term list | 21-27 ms | 2.9-3.1 ms |

Saving the 50,000-term index took 56 to 149 ms before and after.
Before, the app lock was held for that time after every scan.
Now the lock is free during the write, and the write happens only when the scan found a different index.

The first preparation for a new set of terms builds the tables and took 149 to 152 ms for the short text.
The recording flow builds them on a background thread when the recording starts, so a result prepared later in the recording does not wait for them.
A recording stopped before that thread finishes waits for the remainder.

## Limits

- On macOS and Linux a scan trusts a file's size, modification time, and change time; the change time moves when the content, the permissions, or the times of the file are set. A file changed in the two seconds before a scan is always read again. Windows has no change time that Conch reads, so there every scan still reads every file and the scan time does not improve.
- The non-Unix branch of the scan was compiled and its test run only by the Windows job of the desktop check workflow, not on a Windows desktop.
- The characterization test `preparation_of_a_varied_vocabulary_matches_the_recorded_results` pins the prepared text, mask, recognition hint, and suggestions of 22 sentences in four settings against `src-tauri/tests/fixtures/preparation.txt`. The file was recorded from the code before the change. It does not prove equal results for every vocabulary.

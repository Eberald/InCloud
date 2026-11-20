# What's new in InCloud

Each update adds its notes at the top, as a list. After an update InCloud shows the notes
that are new since the version you had, in its welcome window.

## 1.0.1 — November 2026

- When a CLOUDY has no PDF manuals but has their LaTeX sources (docs/latex), the Manuals tab offers to make them: InCloud compiles Hazy 1, 2 and 3 with pdflatex and bibtex, without writing into the CLOUDY folder.
- If TeX is missing, InCloud can install TinyTeX in your home folder (no administrator password), or shows the command for your system's packages; missing TeX packages are installed while compiling.
- README: what the project is for, how it was made, and its licence.

## 1.0 — November 2026

- New Grids tab: a grid of models from ranges of any parameter (steps, steps in dex, lists or commands written by hand), with one input file per model and shell scripts that run them in parallel, here or on a server (bash or SLURM), shared out over several scripts.
- Lines saved with “save lines, cumulative” are read as intensities, no longer integrated a second time (they came out far too strong); “set save prefix” is understood.
- Saved lines on the spectrum stand as high as CLOUDY puts them in its own total spectrum (line flux × ν/Δν of the continuum cell).

- Plots open fitted to their data and can be zoomed out as well as in (− and + over the plot, Ctrl+wheel, *Fit*); saved lines are no longer cut at the top of the spectrum.
- Big folders are searched in the background, with a window that shows how far it got; the list of runs can be filtered.
- Long messages wrap instead of being cut.

- Opening a run finds its files by themselves, whatever their names: from the save commands of the input file or from what each file holds.
- Run info → Files lists what was found, what each file is used for, and the save commands to add for what is missing.
- A folder with several runs (also in sub-folders) asks which one to open.
- “Update available” opens Settings right at the Updates card.
- After an update, the welcome window says what's new; it can be opened again from Settings → Interface.
- InCloud updates itself from git or from its source folder (Settings → Updates).
- The welcome window finds CLOUDY, lets you give its path or installs it for you.
- Fonts and text size of the interface and of the plots, and the plot background, in Settings.
- Checks before a run: InCloud warns when CLOUDY would stop at once or end with warnings, and fixes what it can.
- The spectrum can show the saved lines standing on the total continuum.
- Abundances and reaction rates as tables; any species can be added to the chemical network.
- Run info: why the run stopped, convergence, physical conditions and every warning, explained.
- Manuals searched with Ctrl+F; zoomed plots go back with *Back*.

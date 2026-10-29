// EX-RAIL automation for the Hive-Rail node board; copied into DCC-EX by build.sh.
// The buttons switch the district directly in DCC-EX, so stop works without Hive-Rail.
// DCC-EX then broadcasts <p0>/<p1>; Hive-Rail follows and tells the mesh and LocoNet.

ONBUTTON(15)   // red button (IO15, 10k pull-up): district power off
  POWEROFF
  DONE

ONBUTTON(13)   // green button (IO13, 10k pull-up): district power on
  POWERON
  DONE
